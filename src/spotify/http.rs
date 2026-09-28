use std::time::Duration;

use reqwest::{Client, Response, redirect};

use super::Error;

/// Every Web API and token reply is JSON far below this.
pub const API_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const ERROR_MAX_BYTES: usize = 64 * 1024;

const TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str = concat!(
    "spotypop/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/gbazan92/spotypop)"
);

#[derive(Clone, Debug)]
pub struct Http {
    client: Client,
}

impl Http {
    /// HTTPS only, no proxies from the environment, and no redirects: following
    /// one would replay the bearer token on the new request.
    pub fn new() -> Result<Self, Error> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .https_only(true)
            .no_proxy()
            .redirect(redirect::Policy::none())
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(8))
            .build()?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

/// The whole body, refused as soon as it passes `limit` (checked against
/// Content-Length first, then while reading).
pub async fn read_capped(mut response: Response, limit: usize) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(Error::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > limit {
            return Err(Error::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_builds_with_the_ring_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        assert!(Http::new().is_ok());
    }
}
