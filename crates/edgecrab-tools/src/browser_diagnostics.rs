//! Browser / CDP / localhost diagnostics (single owner for `/browser status` + doctor).
//!
//! First principles: report **flags, proxy, preview policy, session ports, and
//! content class** of known URLs — never collapse into "SSRF or CDP" vibes.

use std::time::Duration;

use crate::dev_server;
use crate::structured_browser::{ContentClass, classify_browser_content};
use crate::tools::browser::{
    HeadlessGpuMode, browser_launch_policy, cdp_override_status, chrome_headless_gpu_args,
    get_chrome_info, get_recording_override, probe_cdp_port,
};

/// Snapshot of browser stack health for operators and doctor.
#[derive(Debug, Clone)]
pub struct BrowserDiagnosticsReport {
    pub mode: String,
    pub endpoint: String,
    pub cdp_reachable: bool,
    pub chrome_browser: Option<String>,
    pub chrome_cdp_version: Option<String>,
    pub headless_gpu: HeadlessGpuMode,
    pub proxy_bypass_loopback: bool,
    pub proxy_url: Option<String>,
    pub recording_on: bool,
    pub preview_enabled: bool,
    pub preview_allow_any: bool,
    pub preview_ports: Vec<u16>,
    pub session_ports: Vec<u16>,
    /// Probed loopback URLs: (url, http_status, content_class label, title).
    pub url_probes: Vec<UrlProbe>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UrlProbe {
    pub url: String,
    pub http_status: Option<u16>,
    pub content_class: String,
    pub title: String,
    pub ok: bool,
}

/// Inputs that live outside the tools crate (preview + session id).
#[derive(Debug, Clone, Default)]
pub struct BrowserDiagnosticsInput {
    pub session_id: Option<String>,
    pub preview_enabled: bool,
    pub preview_allow_any: bool,
    pub preview_ports: Vec<u16>,
    /// Cap how many ports to HTTP-probe (default 4).
    pub max_port_probes: usize,
}

impl Default for BrowserDiagnosticsReport {
    fn default() -> Self {
        Self {
            mode: "unknown".into(),
            endpoint: String::new(),
            cdp_reachable: false,
            chrome_browser: None,
            chrome_cdp_version: None,
            headless_gpu: HeadlessGpuMode::Software,
            proxy_bypass_loopback: true,
            proxy_url: None,
            recording_on: false,
            preview_enabled: false,
            preview_allow_any: false,
            preview_ports: Vec::new(),
            session_ports: Vec::new(),
            url_probes: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// Collect browser diagnostics (async: CDP probe + optional HTTP content probes).
pub async fn collect_browser_diagnostics(
    input: BrowserDiagnosticsInput,
) -> BrowserDiagnosticsReport {
    let policy = browser_launch_policy();
    let proxy_url = edgecrab_security::proxy::resolve_proxy_url(None);
    let recording_on = get_recording_override().unwrap_or(false);

    let (mode, endpoint, host, port) = resolve_mode_endpoint();
    let cdp_reachable = probe_cdp_port(&host, port).await;
    let (chrome_browser, chrome_cdp_version) = if cdp_reachable {
        match get_chrome_info().await {
            Some(ci) => (
                (!ci.browser.is_empty()).then_some(ci.browser),
                (!ci.protocol_version.is_empty()).then_some(ci.protocol_version),
            ),
            None => (None, None),
        }
    } else {
        (None, None)
    };

    let session_ports = input
        .session_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(dev_server::session_http_server_ports)
        .unwrap_or_default();

    let mut ports_to_probe: Vec<u16> = session_ports.clone();
    // Always include common demo ports if empty so doctor still shows something useful.
    if ports_to_probe.is_empty() {
        for p in [8000u16, 8765, 5173, 3000] {
            if !ports_to_probe.contains(&p) {
                ports_to_probe.push(p);
            }
        }
    }
    let max = if input.max_port_probes == 0 {
        4
    } else {
        input.max_port_probes
    };
    ports_to_probe.truncate(max);

    let mut url_probes = Vec::new();
    for p in &ports_to_probe {
        let url = format!("http://127.0.0.1:{p}/");
        url_probes.push(probe_loopback_url(&url).await);
    }

    let mut notes = Vec::new();
    if matches!(policy.headless_gpu, HeadlessGpuMode::Disable) {
        notes.push(
            "headless_gpu=disable — Three.js/WebGL demos often show a blank canvas; \
             set browser.headless_gpu: software"
                .into(),
        );
    }
    if proxy_url.is_some() && !policy.proxy_bypass_loopback {
        notes.push(
            "proxy set without loopback bypass — localhost may fail in CDP Chrome; \
             set browser.proxy_bypass_loopback: true"
                .into(),
        );
    }
    if !input.preview_enabled {
        notes.push(
            "security.preview disabled — browser_navigate to loopback may be blocked (SSRF)."
                .into(),
        );
    }
    if cdp_reachable
        && url_probes
            .iter()
            .any(|p| p.content_class == "http_error_page")
    {
        notes.push(
            "HTTP error page on a session/demo port — wrong process or wrong serve directory \
             (not SSRF). Heal: python3 -m http.server PORT --directory <demo-dir>."
                .into(),
        );
    }
    if !cdp_reachable {
        notes.push(
            "CDP not reachable — run /browser connect or start a session that launches headless Chrome."
                .into(),
        );
    }

    BrowserDiagnosticsReport {
        mode,
        endpoint,
        cdp_reachable,
        chrome_browser,
        chrome_cdp_version,
        headless_gpu: policy.headless_gpu,
        proxy_bypass_loopback: policy.proxy_bypass_loopback,
        proxy_url,
        recording_on,
        preview_enabled: input.preview_enabled,
        preview_allow_any: input.preview_allow_any,
        preview_ports: input.preview_ports,
        session_ports,
        url_probes,
        notes,
    }
}

fn resolve_mode_endpoint() -> (String, String, String, u16) {
    if let Some(ep) = cdp_override_status() {
        let (h, p) = split_host_port(&ep, 9222);
        return ("live CDP override".into(), format!("{h}:{p}"), h, p);
    }
    if let Ok(url) = std::env::var("BROWSER_CDP_URL") {
        let stripped = url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://")
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let (h, p) = split_host_port(stripped.split('/').next().unwrap_or(stripped), 9222);
        return ("BROWSER_CDP_URL env".into(), format!("{h}:{p}"), h, p);
    }
    (
        "local headless Chrome/Chromium".into(),
        "127.0.0.1:9222".into(),
        "127.0.0.1".into(),
        9222,
    )
}

fn split_host_port(s: &str, default_port: u16) -> (String, u16) {
    if let Some((h, p)) = s.rsplit_once(':')
        && let Ok(port) = p.parse::<u16>()
    {
        return (h.to_string(), port);
    }
    if let Ok(port) = s.parse::<u16>() {
        return ("127.0.0.1".into(), port);
    }
    (s.to_string(), default_port)
}

/// HTTP GET loopback with **no proxy** — content class only (title + body markers).
pub async fn probe_loopback_url(url: &str) -> UrlProbe {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::limited(3))
        .no_proxy()
        .build();
    let Ok(client) = client else {
        return UrlProbe {
            url: url.to_string(),
            http_status: None,
            content_class: "transport_fail".into(),
            title: String::new(),
            ok: false,
        };
    };
    match client.get(url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            let title = extract_html_title(&body);
            let class = classify_browser_content(
                Some(url),
                Some(title.as_str()),
                Some(body.as_str()),
                false,
                None,
                status < 400,
            );
            let label = content_class_label(class);
            UrlProbe {
                url: url.to_string(),
                http_status: Some(status),
                content_class: label.into(),
                title,
                ok: class.is_evidence() && status < 400,
            }
        }
        Err(_) => UrlProbe {
            url: url.to_string(),
            http_status: None,
            content_class: "transport_fail".into(),
            title: String::new(),
            ok: false,
        },
    }
}

fn extract_html_title(body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    let Some(start) = lower.find("<title") else {
        return String::new();
    };
    let after = &body[start..];
    let Some(gt) = after.find('>') else {
        return String::new();
    };
    let rest = &after[gt + 1..];
    let Some(end) = rest.to_ascii_lowercase().find("</title>") else {
        return String::new();
    };
    rest[..end].trim().chars().take(120).collect()
}

fn content_class_label(c: ContentClass) -> &'static str {
    match c {
        ContentClass::Ok => "ok",
        ContentClass::ChromeError => "chrome_error",
        ContentClass::HttpErrorPage => "http_error_page",
        ContentClass::EmptyDocument => "empty_document",
        ContentClass::TransportFail => "transport_fail",
        ContentClass::AppFail => "app_fail",
    }
}

/// Human-readable multi-line report for TUI `/browser status` and CLI doctor.
pub fn format_browser_diagnostics(report: &BrowserDiagnosticsReport) -> String {
    let mut lines = String::new();
    lines.push_str("🌐 Browser diagnostics\n");
    lines.push_str(&format!("   Mode:      {}\n", report.mode));
    lines.push_str(&format!("   Endpoint:  {}\n", report.endpoint));
    lines.push_str(&format!(
        "   CDP:       {}\n",
        if report.cdp_reachable {
            "✓ reachable"
        } else {
            "✗ not reachable"
        }
    ));
    if let Some(ref b) = report.chrome_browser {
        lines.push_str(&format!("   Chrome:    {b}\n"));
    }
    if let Some(ref v) = report.chrome_cdp_version {
        lines.push_str(&format!("   CDP ver:   {v}\n"));
    }

    let gpu = match report.headless_gpu {
        HeadlessGpuMode::Software => "software (ANGLE/SwiftShader, WebGL on)",
        HeadlessGpuMode::Disable => "disable (--disable-gpu; blank canvas risk)",
    };
    lines.push_str(&format!("   GPU mode:  {gpu}\n"));
    let gpu_flags = chrome_headless_gpu_args(report.headless_gpu);
    if !gpu_flags.is_empty() {
        lines.push_str(&format!("   GPU flags: {}\n", gpu_flags.join(" ")));
    }

    match &report.proxy_url {
        Some(p) => {
            lines.push_str(&format!("   Proxy:     {p}\n"));
            lines.push_str(&format!(
                "   Bypass:    loopback {}\n",
                if report.proxy_bypass_loopback {
                    "✓ on (<-loopback>;localhost;127.0.0.1)"
                } else {
                    "✗ off (localhost may break)"
                }
            ));
        }
        None => lines.push_str("   Proxy:     (none)\n"),
    }

    lines.push_str(&format!(
        "   Recording: {}\n",
        if report.recording_on { "on" } else { "off" }
    ));

    lines.push_str(&format!(
        "   Preview:   {}{}\n",
        if report.preview_enabled {
            "enabled"
        } else {
            "DISABLED"
        },
        if report.preview_allow_any {
            " · any loopback port"
        } else {
            ""
        }
    ));
    if !report.preview_allow_any && !report.preview_ports.is_empty() {
        lines.push_str(&format!("   Ports:     {:?}\n", report.preview_ports));
    }

    if report.session_ports.is_empty() {
        lines.push_str("   Session HTTP ports: (none recorded)\n");
    } else {
        lines.push_str(&format!(
            "   Session HTTP ports: {:?}\n",
            report.session_ports
        ));
    }

    if !report.url_probes.is_empty() {
        lines.push_str("   URL content probes (no proxy):\n");
        for p in &report.url_probes {
            let st = p
                .http_status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "—".into());
            let mark = if p.ok { "✓" } else { "✗" };
            let title = if p.title.is_empty() {
                "(no title)"
            } else {
                p.title.as_str()
            };
            lines.push_str(&format!(
                "     {mark} {} → HTTP {st} · class={} · {title}\n",
                p.url, p.content_class
            ));
        }
    }

    if !report.notes.is_empty() {
        lines.push_str("   Notes:\n");
        for n in &report.notes {
            lines.push_str(&format!("     • {n}\n"));
        }
    }

    lines.push('\n');
    lines.push_str("   /browser connect [ws://host:port]  — live Chrome\n");
    lines.push_str("   /browser disconnect               — headless default\n");
    lines.push_str("   /browser tabs                     — list CDP tabs\n");
    lines.push_str("   /browser recording on|off         — session recording\n");
    lines.push_str("   edgecrab doctor                   — full stack check\n");
    lines
}

/// Compact one-line summary for doctor check table.
pub fn browser_diagnostics_doctor_detail(report: &BrowserDiagnosticsReport) -> (bool, String) {
    let ok = report.cdp_reachable
        || report
            .url_probes
            .iter()
            .any(|p| p.ok || p.http_status.is_some());
    let proxy = report
        .proxy_url
        .as_ref()
        .map(|p| format!("proxy={p}"))
        .unwrap_or_else(|| "proxy=none".into());
    let gpu = match report.headless_gpu {
        HeadlessGpuMode::Software => "gpu=software",
        HeadlessGpuMode::Disable => "gpu=disable",
    };
    let cdp = if report.cdp_reachable {
        "cdp=up"
    } else {
        "cdp=down"
    };
    let preview = if report.preview_enabled {
        "preview=on"
    } else {
        "preview=off"
    };
    let ports = if report.session_ports.is_empty() {
        "ports=none".into()
    } else {
        format!("ports={:?}", report.session_ports)
    };
    let probe = report
        .url_probes
        .iter()
        .find(|p| p.http_status.is_some())
        .map(|p| format!("{}:{}", p.content_class, p.http_status.unwrap_or(0)))
        .unwrap_or_else(|| "probe=n/a".into());
    (
        ok || report.preview_enabled,
        format!("{cdp} · {gpu} · {proxy} · {preview} · {ports} · {probe}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_report_includes_sections() {
        let report = BrowserDiagnosticsReport {
            mode: "local headless".into(),
            endpoint: "127.0.0.1:9222".into(),
            cdp_reachable: false,
            headless_gpu: HeadlessGpuMode::Software,
            proxy_bypass_loopback: true,
            preview_enabled: true,
            preview_allow_any: true,
            url_probes: vec![UrlProbe {
                url: "http://127.0.0.1:8000/".into(),
                http_status: Some(200),
                content_class: "ok".into(),
                title: "3D Chess".into(),
                ok: true,
            }],
            notes: vec!["test note".into()],
            ..Default::default()
        };
        let text = format_browser_diagnostics(&report);
        assert!(text.contains("Browser diagnostics"));
        assert!(text.contains("GPU mode"));
        assert!(text.contains("content probes"));
        assert!(text.contains("3D Chess"));
        assert!(text.contains("test note"));
    }

    #[test]
    fn extract_title_basic() {
        let t = extract_html_title("<html><title>♔ 3D Chess</title></html>");
        assert!(t.contains("3D Chess"));
    }

    #[test]
    fn doctor_detail_line() {
        let report = BrowserDiagnosticsReport {
            cdp_reachable: true,
            headless_gpu: HeadlessGpuMode::Software,
            preview_enabled: true,
            session_ports: vec![8000],
            url_probes: vec![UrlProbe {
                url: "http://127.0.0.1:8000/".into(),
                http_status: Some(404),
                content_class: "http_error_page".into(),
                title: "Error response".into(),
                ok: false,
            }],
            ..Default::default()
        };
        let (ok, detail) = browser_diagnostics_doctor_detail(&report);
        assert!(ok);
        assert!(detail.contains("cdp=up"));
        assert!(detail.contains("http_error_page"));
    }

    #[test]
    fn split_host_port_works() {
        assert_eq!(
            split_host_port("127.0.0.1:9333", 9222),
            ("127.0.0.1".into(), 9333)
        );
        assert_eq!(split_host_port("9222", 1), ("127.0.0.1".into(), 9222));
    }

    #[test]
    fn policy_default_is_constructible() {
        let _ = browser_launch_policy();
    }
}
