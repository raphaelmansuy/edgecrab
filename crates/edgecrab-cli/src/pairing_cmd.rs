use std::path::PathBuf;

use anyhow::Context;
use edgecrab_gateway::pairing::PairingStore;

use crate::cli_args::PairingCommand;

pub fn run(command: PairingCommand) -> anyhow::Result<()> {
    let store = PairingStore::new();
    match command {
        PairingCommand::List {
            pending,
            approved,
            platform,
        } => list_pairings(&store, pending, approved, platform.as_deref()),
        PairingCommand::Approve { platform, code } => approve_pairing(&store, &platform, &code),
        PairingCommand::Revoke { platform, user_id } => revoke_pairing(&store, &platform, &user_id),
        PairingCommand::ClearPending { platform } => clear_pending(platform.as_deref()),
    }
}

fn list_pairings(
    store: &PairingStore,
    pending_only: bool,
    approved_only: bool,
    platform: Option<&str>,
) -> anyhow::Result<()> {
    let show_pending = pending_only || !approved_only;
    let show_approved = approved_only || !pending_only;

    if show_pending {
        let pending = store.list_pending(platform);
        if pending.is_empty() {
            println!("Pending pairing requests: none");
        } else {
            println!("Pending pairing requests:");
            for (platform, code, user_id, user_name, age_min) in pending {
                println!(
                    "  {:10} code={} user={} ({}) age={}m",
                    platform,
                    code,
                    user_id,
                    display_name(&user_name),
                    age_min
                );
            }
        }
    }

    if show_approved {
        if show_pending {
            println!();
        }
        let approved = store.list_approved(platform);
        if approved.is_empty() {
            println!("Approved pairings: none");
        } else {
            println!("Approved pairings:");
            for (platform, user_id, user_name) in approved {
                println!(
                    "  {:10} user={} ({})",
                    platform,
                    user_id,
                    display_name(&user_name)
                );
            }
        }
    }

    Ok(())
}

fn approve_pairing(store: &PairingStore, platform: &str, code: &str) -> anyhow::Result<()> {
    match store.approve_code(platform, code) {
        Some((user_id, user_name)) => {
            println!(
                "Approved pairing for {} user {} ({}).",
                platform,
                user_id,
                display_name(&user_name)
            );
            Ok(())
        }
        None => anyhow::bail!("No pending pairing code '{code}' for platform '{platform}'."),
    }
}

fn revoke_pairing(store: &PairingStore, platform: &str, user_id: &str) -> anyhow::Result<()> {
    if store.revoke(platform, user_id) {
        println!("Revoked pairing for {} user {}.", platform, user_id);
        Ok(())
    } else {
        anyhow::bail!(
            "User '{}' is not currently paired on '{}'.",
            user_id,
            platform
        );
    }
}

fn clear_pending(platform: Option<&str>) -> anyhow::Result<()> {
    let files = pending_files(platform)?;
    if files.is_empty() {
        println!("No pending pairing files found.");
        return Ok(());
    }

    for path in &files {
        if path.exists() {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }

    println!("Cleared {} pending pairing file(s).", files.len());
    Ok(())
}

fn pending_files(platform: Option<&str>) -> anyhow::Result<Vec<PathBuf>> {
    let dir = edgecrab_core::edgecrab_home().join("pairing");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    if let Some(platform) = platform {
        return Ok(vec![dir.join(format!("{platform}-pending.json"))]);
    }

    let mut files = Vec::new();
    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with("-pending.json"))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn display_name(name: &str) -> &str {
    if name.trim().is_empty() { "-" } else { name }
}
