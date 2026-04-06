use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use edgecrab_types::ToolError;

use crate::execution_fs::effective_docker_workspace_mount;
use crate::registry::ToolContext;
use crate::tools::backends::{BackendConfig, ExecutionBackend, build_backend};

pub(crate) struct BackendCacheEntry {
    pub(crate) backend: Arc<dyn ExecutionBackend>,
    pub(crate) last_used_epoch_secs: AtomicU64,
}

impl BackendCacheEntry {
    pub(crate) fn new(backend: Arc<dyn ExecutionBackend>) -> Self {
        Self {
            backend,
            last_used_epoch_secs: AtomicU64::new(now_epoch_secs()),
        }
    }

    fn touch(&self) {
        self.last_used_epoch_secs
            .store(now_epoch_secs(), Ordering::Relaxed);
    }

    fn idle_for_secs(&self, now_epoch_secs: u64) -> u64 {
        now_epoch_secs.saturating_sub(self.last_used_epoch_secs.load(Ordering::Relaxed))
    }
}

pub(crate) fn backend_cache() -> &'static DashMap<String, Arc<BackendCacheEntry>> {
    static CACHE: OnceLock<DashMap<String, Arc<BackendCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

pub(crate) fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn prepare_backend_config(ctx: &ToolContext) -> BackendConfig {
    let mut docker = ctx.config.terminal_docker.clone();

    if ctx.config.terminal_backend == crate::tools::backends::BackendKind::Docker
        && docker.workspace_mount.is_none()
    {
        docker.workspace_mount = Some(effective_docker_workspace_mount(&ctx.cwd, &docker));
    }

    BackendConfig {
        kind: ctx.config.terminal_backend.clone(),
        task_id: ctx.task_id.clone(),
        docker,
        ssh: ctx.config.terminal_ssh.clone(),
        modal: ctx.config.terminal_modal.clone(),
        daytona: ctx.config.terminal_daytona.clone(),
        singularity: ctx.config.terminal_singularity.clone(),
    }
}

fn backend_idle_ttl() -> Duration {
    std::env::var("EDGECRAB_TERMINAL_BACKEND_IDLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(300))
}

fn backend_cleanup_interval() -> Duration {
    std::env::var("EDGECRAB_TERMINAL_BACKEND_SWEEP_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(60))
}

fn last_cleanup_epoch_secs() -> &'static AtomicU64 {
    static LAST_SWEEP: OnceLock<AtomicU64> = OnceLock::new();
    LAST_SWEEP.get_or_init(|| AtomicU64::new(0))
}

pub async fn cleanup_inactive_backends(max_idle: Duration) -> usize {
    let now = now_epoch_secs();
    let max_idle_secs = max_idle.as_secs();
    let mut stale = Vec::new();

    backend_cache().retain(|task_id, entry| {
        let idle_secs = entry.idle_for_secs(now);
        let cache_is_last_owner = Arc::strong_count(&entry.backend) == 1;
        let should_remove = idle_secs >= max_idle_secs && cache_is_last_owner;
        if should_remove {
            stale.push((
                task_id.clone(),
                entry.backend.clone(),
                entry.backend.kind(),
                idle_secs,
            ));
        }
        !should_remove
    });

    for (task_id, backend, kind, idle_secs) in &stale {
        tracing::info!(
            task_id = %task_id,
            kind = %kind,
            idle_secs = *idle_secs,
            "cleaning up inactive terminal backend"
        );
        let _ = backend.cleanup().await;
    }

    stale.len()
}

pub async fn cleanup_all_backends() -> usize {
    let mut stale = Vec::new();
    backend_cache().retain(|task_id, entry| {
        stale.push((task_id.clone(), entry.backend.clone(), entry.backend.kind()));
        false
    });

    for (task_id, backend, kind) in &stale {
        tracing::info!(
            task_id = %task_id,
            kind = %kind,
            "cleaning up terminal backend on shutdown"
        );
        let _ = backend.cleanup().await;
    }

    stale.len()
}

pub async fn cleanup_backend_for_task(task_id: &str) -> bool {
    let Some((_, entry)) = backend_cache().remove(task_id) else {
        return false;
    };
    tracing::info!(
        task_id = %task_id,
        kind = %entry.backend.kind(),
        "cleaning up terminal backend for task"
    );
    let _ = entry.backend.cleanup().await;
    true
}

async fn maybe_cleanup_inactive_backends() {
    let now = now_epoch_secs();
    let interval_secs = backend_cleanup_interval().as_secs();
    let last = last_cleanup_epoch_secs().load(Ordering::Relaxed);
    if now.saturating_sub(last) < interval_secs {
        return;
    }

    last_cleanup_epoch_secs().store(now, Ordering::Relaxed);
    let _ = cleanup_inactive_backends(backend_idle_ttl()).await;
}

pub(crate) async fn get_or_create_backend(
    ctx: &ToolContext,
) -> Result<Arc<dyn ExecutionBackend>, ToolError> {
    maybe_cleanup_inactive_backends().await;
    let cache = backend_cache();

    if let Some(existing) = cache.get(&ctx.task_id) {
        let entry = existing.clone();
        drop(existing);
        entry.touch();
        let backend = entry.backend.clone();
        if backend.is_healthy().await {
            return Ok(backend);
        }
        tracing::warn!(
            task_id = %ctx.task_id,
            kind = %backend.kind(),
            "cached backend is unhealthy; evicting and rebuilding"
        );
        let _ = backend.cleanup().await;
        cache.remove(&ctx.task_id);
    }

    let backend: Arc<dyn ExecutionBackend> =
        Arc::from(build_backend(prepare_backend_config(ctx)).await?);
    cache.insert(
        ctx.task_id.clone(),
        Arc::new(BackendCacheEntry::new(backend.clone())),
    );
    Ok(backend)
}

pub(crate) fn resolve_workdir(ctx: &ToolContext, workdir: Option<&str>) -> String {
    match workdir {
        Some(wd) => {
            let p = std::path::Path::new(wd);
            if p.is_absolute() {
                wd.to_string()
            } else {
                ctx.cwd.join(p).to_string_lossy().into_owned()
            }
        }
        None => ctx.cwd.to_string_lossy().into_owned(),
    }
}
