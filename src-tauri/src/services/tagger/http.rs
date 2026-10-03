use crate::error::{CrateError, Result};

/// Chrome User-Agent presented to the metadata providers (Beatport rejects
/// default clients; the system `curl` request to TraxSource reuses it).
pub(super) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

/// Build the shared HTTP client for the `reqwest`-based tagger providers:
/// Beatport (bearer-token API) and Bandcamp (public JSON).
///
/// TraxSource does not use this client — Cloudflare blocks `reqwest`'s
/// fingerprint, so it shells out to the system `curl` instead (see `traxsource`).
pub(super) fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| CrateError::Tagger(format!("Failed to create HTTP client: {e}")))
}
