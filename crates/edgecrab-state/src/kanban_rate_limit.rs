//! Rate-limit detection for kanban worker failures (shared by state + core).

/// Whether a worker error looks like a provider quota / rate-limit wall.
pub fn is_rate_limit_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("rate_limit")
        || lower.contains("rate limit")
        || lower.contains("rate-limit")
        || lower.contains("quota")
        || lower.contains("too many requests")
}
