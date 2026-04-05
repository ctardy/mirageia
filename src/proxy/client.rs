use bytes::Bytes;
use http::HeaderMap;
use reqwest::Client;

use super::error::ProxyError;

pub struct UpstreamClient {
    client: Client,
}

impl UpstreamClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .build()
            .expect("Impossible de créer le client HTTP");

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
