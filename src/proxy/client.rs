use bytes::Bytes;
use http::HeaderMap;
use reqwest::Client;

use super::error::ProxyError;

pub struct UpstreamClient {
    client: Client,
}

impl UpstreamClient {
    pub fn new(upstream_proxy: Option<&str>, accept_invalid_certs: bool) -> Self {
        let mut builder = Client::builder();

        if let Some(proxy_url) = upstream_proxy {
            match reqwest::Proxy::all(proxy_url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                    tracing::info!("Upstream proxy configured: {}", proxy_url);
                }
                Err(e) => {
                    tracing::warn!("Invalid upstream_proxy URL '{}': {} — ignored", proxy_url, e);
                }
            }
        }

        if accept_invalid_certs {
            tracing::warn!("TLS certificate validation disabled (danger_accept_invalid_certs=true)");
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    /// Sends a request to the upstream and returns the raw response (for streaming or not).
    pub async fn forward(
        &self,
        method: http::Method,
        url: &str,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<reqwest::Response, ProxyError> {
        let response = self
            .client
            .request(method, url)
            .headers(headers)
            .body(body)
            .send()
            .await?;

        Ok(response)
    }
}
