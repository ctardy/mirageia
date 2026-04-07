use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::error::Error as StdError;

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("Erreur upstream : {0}")]
    Upstream(#[from] reqwest::Error),

    #[error("Erreur HTTP : {0}")]
    Http(String),

    #[error("Provider inconnu pour le path : {0}")]
    UnknownProvider(String),

    #[error("Erreur de lecture du body : {0}")]
    Body(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        // Include root cause for better diagnostics
        if let ProxyError::Upstream(ref e) = self {
            let cause = e.source()
                .map(|s| format!(" — cause: {}", s))
                .unwrap_or_default();
            tracing::error!("Proxy error: {}{}", self, cause);
        } else {
            tracing::error!("Proxy error: {}", self);
        }

        let status = match &self {
            ProxyError::Upstream(_) => StatusCode::BAD_GATEWAY,
            ProxyError::Http(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ProxyError::UnknownProvider(_) => StatusCode::NOT_FOUND,
            ProxyError::Body(_) => StatusCode::BAD_REQUEST,
        };

        let body = serde_json::json!({
            "error": {
                "type": "mirageia_error",
                "message": self.to_string(),
            }
        });

        (status, axum::Json(body)).into_response()
    }
}
