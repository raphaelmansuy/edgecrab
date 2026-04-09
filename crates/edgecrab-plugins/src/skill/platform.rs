pub fn skill_matches_platform(platforms: &[String]) -> bool {
    if platforms.is_empty() {
        return true;
    }
    let os = std::env::consts::OS;
    let hermes_os = match os {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        other => other,
    };
    platforms.iter().any(|platform| {
        let normalized_platform = platform.to_ascii_lowercase();
        let normalized = match normalized_platform.as_str() {
            "macos" => "darwin",
            "linux" => "linux",
            "windows" => "win32",
            other => other,
        };
        normalized == hermes_os
    })
}
