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

    /// Envoie une requête à l'upstream et retourne la réponse brute (pour streaming ou non).
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
