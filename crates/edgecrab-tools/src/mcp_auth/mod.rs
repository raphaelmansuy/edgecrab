//! MCP Authorization discovery (RFC 9728 / RFC 8414 / RFC 7591).
//!
//! Enables URL-only OAuth setup for remote HTTP MCP servers per the MCP
//! Authorization specification (2025-11-25 / 2026-07-28).

mod dcr;
mod discovery;

pub use dcr::{
    DEFAULT_MCP_REDIRECT_URIS, DEFAULT_MCP_REDIRECT_URL, DcrClient, DcrRequest,
    register_oauth_client,
};
pub use discovery::{
    AsMetadata, DiscoverError, DiscoverOpts, DiscoveredMcpOauth, WwwAuthenticateChallenge,
    as_metadata_urls, discover_mcp_oauth, parse_www_authenticate, prm_candidate_urls,
    suggest_server_name,
};
