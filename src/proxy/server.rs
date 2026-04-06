use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use bytes::Bytes;
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::config::AppConfig;
use crate::detection::regex_detector::RegexDetector;
#[cfg(feature = "onnx")]
use crate::detection::PiiDetector;
use crate::extraction::preprocess_media_blocks;
use crate::mapping::MappingTable;
use crate::pseudonymization::generator::PseudonymGenerator;
use crate::pseudonymization::{depseudonymize_text, pseudonymize_text};
use crate::proxy::extractor::{extract_text_fields, rebuild_body, JsonPath};
use crate::streaming::{parse_sse_chunk, rebuild_sse_chunk, StreamBuffer};

use super::client::UpstreamClient;
use super::error::ProxyError;
use super::router;

/// Direction of a proxy event (request or response).
#[derive(Debug, Clone)]
pub enum Direction {
    Request,
    Response,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Request => write!(f, "→"),
            Direction::Response => write!(f, "←"),
        }
    }
}

/// Event emitted by the proxy for the monitoring console.
#[derive(Debug, Clone)]
pub struct ProxyEvent {
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub provider: String,
    pub path: String,
    pub direction: Direction,
    pub pii_count: usize,
    pub passthrough: bool,
    pub body_size: usize,
    pub model: Option<String>,
    pub pii_types: Vec<String>,
    pub status_code: Option<u16>,
    pub duration_ms: Option<u64>,
    pub streaming: Option<bool>,
}

impl std::fmt::Display for ProxyEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let time = self.timestamp.format("%H:%M:%S");
        let mode = if self.passthrough { "PASS" } else { "PII " };
        let pii = if self.pii_count > 0 {
            format!(" ({} PII)", self.pii_count)
        } else {
            String::new()
        };
        write!(
            f,
            "[{}] {} {} {:<10} {}{}",
            time, self.direction, mode, self.provider, self.path, pii
        )
    }
}

/// Extracts the model name from the JSON body of an LLM request.
fn extract_model_from_body(body_bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body_bytes)
        .ok()
        .and_then(|v| v.get("model")?.as_str().map(String::from))
}

pub struct ProxyState {
    pub config: AppConfig,
    pub client: UpstreamClient,
    pub detector: RegexDetector,
    #[cfg(feature = "onnx")]
    pub onnx_detector: Option<Arc<PiiDetector>>,
    /// Name of the loaded ONNX model, or None if regex-only mode.
    pub onnx_model: Option<String>,
    pub mapping: Arc<MappingTable>,
    pub generator: Arc<Mutex<PseudonymGenerator>>,
    pub events_tx: broadcast::Sender<ProxyEvent>,
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
}

/// Creates a ProxyState from a config (for tests).
pub fn create_state(config: AppConfig) -> ProxyState {
    let (events_tx, _) = broadcast::channel(256);
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);

    #[cfg(feature = "onnx")]
    let (onnx_detector, onnx_model) = load_onnx_detector(&config);
    #[cfg(not(feature = "onnx"))]
    let onnx_model: Option<String> = None;

    ProxyState {
        config,
        client: UpstreamClient::new(),
        detector: RegexDetector::new(),
        #[cfg(feature = "onnx")]
        onnx_detector,
        onnx_model,
        mapping: Arc::new(MappingTable::new()),
        generator: Arc::new(Mutex::new(PseudonymGenerator::new())),
        events_tx,
        shutdown_tx,
    }
}

/// Tente de charger le PiiDetector ONNX depuis le modèle configuré ou le modèle actif.
/// Retourne None si aucun modèle n'est disponible ou si le chargement échoue (fail-open).
#[cfg(feature = "onnx")]
fn load_onnx_detector(config: &AppConfig) -> (Option<Arc<PiiDetector>>, Option<String>) {
    use crate::detection::model_manager;

    let Some(model_name) = config.model_name.clone().or_else(model_manager::get_active_model) else {
        return (None, None);
    };

    match PiiDetector::from_model_name(&model_name) {
        Ok(detector) => {
            tracing::info!("Modèle ONNX '{}' chargé — détection contextuelle active", model_name);
            (Some(Arc::new(detector)), Some(model_name))
        }
        Err(e) => {
            tracing::warn!("Modèle ONNX '{}' non disponible : {} — détection regex seule", model_name, e);
            (None, None)
        }
    }
}

/// Returns the handler as a fallback for an axum Router (for tests).
pub fn create_router(state: Arc<ProxyState>) -> Router {
    Router::new().fallback(proxy_handler).with_state(state)
}

/// Starts the HTTP proxy on the configured address.
pub async fn start_proxy(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(create_state(config.clone()));
    let mut shutdown_rx = state.shutdown_tx.subscribe();

    let onnx_info = state.onnx_model.as_deref()
        .map(|m| format!(" | ONNX: {}", m))
        .unwrap_or_else(|| " | ONNX: off (regex only)".to_string());

    let app = create_router(state);

    let listener = TcpListener::bind(&config.listen_addr).await?;
    if config.passthrough {
        tracing::info!(
            "MirageIA proxy écoute sur {} (MODE PASSTHROUGH — pas de pseudonymisation){}",
            config.listen_addr, onnx_info
        );
    } else {
        tracing::info!("MirageIA proxy écoute sur {}{}", config.listen_addr, onnx_info);
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.wait_for(|&v| v).await;
            tracing::info!("Arrêt gracieux du proxy...");
        })
        .await?;

    Ok(())
}

/// Headers to copy from the incoming request to the upstream.
const FORWARDED_HEADERS: &[&str] = &[
    "content-type",
    "x-api-key",
    "authorization",
    "anthropic-version",
    "anthropic-beta",
    "accept",
    "user-agent",
];

/// Catch-all handler: receives any request, routes it to the correct provider, relays the response.
async fn proxy_handler(
    State(state): State<Arc<ProxyState>>,
    request: Request<Body>,
) -> Result<Response, ProxyError> {
    let path = request.uri().path().to_string();

    // Graceful proxy shutdown
    if path == "/shutdown" && request.method() == http::Method::POST {
        tracing::info!("Arrêt du proxy demandé via /shutdown");
        let _ = state.shutdown_tx.send(true);
        return Ok((StatusCode::OK, "MirageIA arrêté\n").into_response());
    }

    // Health check
    if path == "/health" {
        let stats = serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "passthrough": state.config.passthrough,
            "pii_mappings": state.mapping.len(),
            "onnx_model": state.onnx_model,
        });
        return Ok((StatusCode::OK, axum::Json(stats)).into_response());
    }

    // SSE endpoint for the monitoring console
    if path == "/events" {
        let mut rx = state.events_tx.subscribe();
        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let data = serde_json::json!({
                            "timestamp": event.timestamp.to_rfc3339(),
                            "provider": event.provider,
                            "path": event.path,
                            "direction": format!("{}", event.direction),
                            "pii_count": event.pii_count,
                            "passthrough": event.passthrough,
                            "body_size": event.body_size,
                            "model": event.model,
                            "pii_types": event.pii_types,
                            "status_code": event.status_code,
                            "duration_ms": event.duration_ms,
                            "streaming": event.streaming,
                        });
                        yield Ok::<_, std::io::Error>(
                            format!("data: {}\n\n", data)
                        );
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        };
        let body = Body::from_stream(stream);
        return Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .body(body)
            .map_err(|e| ProxyError::Http(e.to_string()));
    }

    // Web dashboard
    if path == "/dashboard" {
        return Response::builder()
            .status(200)
            .header("content-type", "text/html; charset=utf-8")
            .body(Body::from(DASHBOARD_HTML))
            .map_err(|e| ProxyError::Http(e.to_string()));
    }

    // Resolve the provider
    let provider = router::resolve_provider(&path)
        .ok_or_else(|| ProxyError::UnknownProvider(path.clone()))?;

    // Bearer token authentication (optional — only if proxy_token is configured)
    if let Some(expected_token) = &state.config.proxy_token {
        let authorized = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|token| token == expected_token)
            .unwrap_or(false);

        if !authorized {
            tracing::warn!("Unauthorized request rejected — missing or invalid bearer token");
            let error_body = serde_json::json!({
                "error": "unauthorized",
                "message": "Bearer token required",
            });
            return Ok((StatusCode::UNAUTHORIZED, axum::Json(error_body)).into_response());
        }
    }

    let upstream_url = router::upstream_url(provider, &path, &state.config)
        .map_err(ProxyError::Http)?;

    // Copy relevant headers
    let mut upstream_headers = http::HeaderMap::new();
    for &name in FORWARDED_HEADERS {
        if let Some(value) = request.headers().get(name) {
            if let Ok(header_name) = http::header::HeaderName::from_bytes(name.as_bytes()) {
                upstream_headers.insert(header_name, value.clone());
            }
        }
    }

    // Read the body
    let method = request.method().clone();
    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Body(e.to_string()))?;

    let model = extract_model_from_body(&body_bytes);
    let request_body_size = body_bytes.len();

    // --- PASSTHROUGH MODE: relay without pseudonymization ---
    if state.config.passthrough {
        tracing::debug!(
            provider = ?provider,
            url = %upstream_url,
            "Mode passthrough — requête relayée sans pseudonymisation"
        );

        let _ = state.events_tx.send(ProxyEvent {
            timestamp: chrono::Local::now(),
            provider: format!("{:?}", provider),
            path: path.clone(),
            direction: Direction::Request,
            pii_count: 0,
            passthrough: true,
            body_size: request_body_size,
            model: model.clone(),
            pii_types: Vec::new(),
            status_code: None,
            duration_ms: None,
            streaming: None,
        });

        let start = std::time::Instant::now();
        let upstream_response = state
            .client
            .forward(method, &upstream_url, upstream_headers, Bytes::from(body_bytes.to_vec()))
            .await?;
        let duration = start.elapsed().as_millis() as u64;

        let status_code = upstream_response.status().as_u16();
        let is_sse = upstream_response.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false);

        let _ = state.events_tx.send(ProxyEvent {
            timestamp: chrono::Local::now(),
            provider: format!("{:?}", provider),
            path: path.clone(),
            direction: Direction::Response,
            pii_count: 0,
            passthrough: true,
            body_size: 0,
            model: None,
            pii_types: Vec::new(),
            status_code: Some(status_code),
            duration_ms: Some(duration),
            streaming: Some(is_sse),
        });

        return build_passthrough_response(upstream_response).await;
    }

    // --- REQUEST PSEUDONYMIZATION (fail-open) ---
    let pseudo_result = match pseudonymize_request(&body_bytes, provider, &state) {
        Ok(result) => {
            if result.was_pseudonymized {
                tracing::info!(
                    provider = ?provider,
                    mappings = state.mapping.len(),
                    "Requête pseudonymisée"
                );
            }
            result
        }
        Err(e) => {
            if state.config.fail_open {
                tracing::warn!(
                    "[SECURITY WARNING] Pseudonymization failed — forwarding unmasked request: {}",
                    e
                );
                PseudonymizeResult {
                    body: body_bytes.to_vec(),
                    was_pseudonymized: false,
                    pii_count: 0,
                    pii_types: Vec::new(),
                }
            } else {
                let error_body = serde_json::json!({
                    "error": "pseudonymization_failed",
                    "message": format!("{}", e),
                });
                let response = (
                    StatusCode::BAD_GATEWAY,
                    axum::Json(error_body),
                )
                    .into_response();
                return Ok(response);
            }
        }
    };

    let final_body = pseudo_result.body;
    let was_pseudonymized = pseudo_result.was_pseudonymized;
    let pii_count = pseudo_result.pii_count;

    let _ = state.events_tx.send(ProxyEvent {
        timestamp: chrono::Local::now(),
        provider: format!("{:?}", provider),
        path: path.clone(),
        direction: Direction::Request,
        pii_count,
        passthrough: false,
        body_size: request_body_size,
        model: model.clone(),
        pii_types: pseudo_result.pii_types,
        status_code: None,
        duration_ms: None,
        streaming: None,
    });

    tracing::debug!(
        provider = ?provider,
        url = %upstream_url,
        body_size = final_body.len(),
        pseudonymized = was_pseudonymized,
        "Forwarding requête"
    );

    // Send to upstream
    let start = std::time::Instant::now();
    let upstream_response = state
        .client
        .forward(method, &upstream_url, upstream_headers, Bytes::from(final_body))
        .await?;
    let duration = start.elapsed().as_millis() as u64;

    let status_code = upstream_response.status().as_u16();
    let is_sse = upstream_response.headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false);

    let _ = state.events_tx.send(ProxyEvent {
        timestamp: chrono::Local::now(),
        provider: format!("{:?}", provider),
        path: path.clone(),
        direction: Direction::Response,
        pii_count: 0,
        passthrough: false,
        body_size: 0,
        model: None,
        pii_types: Vec::new(),
        status_code: Some(status_code),
        duration_ms: Some(duration),
        streaming: Some(is_sse),
    });

    // --- RESPONSE DE-PSEUDONYMIZATION ---
    if was_pseudonymized {
        build_depseudonymized_response(upstream_response, provider, &state).await
    } else {
        build_passthrough_response(upstream_response).await
    }
}

/// Result of request pseudonymization.
struct PseudonymizeResult {
    body: Vec<u8>,
    was_pseudonymized: bool,
    pii_count: usize,
    pii_types: Vec<String>,
}

/// Pseudonymizes the request body.
fn pseudonymize_request(
    body_bytes: &[u8],
    provider: router::Provider,
    state: &ProxyState,
) -> Result<PseudonymizeResult, String> {
    let mut body: serde_json::Value =
        serde_json::from_slice(body_bytes).map_err(|e| format!("JSON invalide : {}", e))?;

    // Preprocessing: convert document blocks (PDF, DOCX) to text blocks
    let media_converted = preprocess_media_blocks(&mut body, provider);
    if media_converted > 0 {
        tracing::debug!(media_converted, "Blocs media convertis en texte");
    }

    let text_fields = extract_text_fields(&body, provider);

    if text_fields.is_empty() {
        return Ok(PseudonymizeResult {
            body: body_bytes.to_vec(),
            was_pseudonymized: false,
            pii_count: 0,
            pii_types: Vec::new(),
        });
    }

    let mut replacements: Vec<(JsonPath, String)> = Vec::new();
    let mut total_pii = 0;
    let mut pii_type_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    let generator = state.generator.lock().unwrap();

    for field in &text_fields {
        let entities = state.detector.detect_with_whitelist(&field.text, &state.config.whitelist);

        // Enrich with ONNX contextual detection (names, dates, addresses, etc.)
        // Shadow `entities` to append ONNX entities that don't overlap regex ones.
        #[cfg(feature = "onnx")]
        let entities = {
            let mut combined = entities;
            if let Some(onnx) = &state.onnx_detector {
                if let Ok(onnx_entities) = onnx.detect(&field.text) {
                    for onnx_entity in onnx_entities {
                        // Skip Unknown labels and anything already covered by regex
                        if onnx_entity.entity_type == crate::detection::PiiType::Unknown {
                            continue;
                        }
                        let overlaps = combined.iter().any(|e| {
                            onnx_entity.start < e.end && onnx_entity.end > e.start
                        });
                        if !overlaps {
                            combined.push(onnx_entity);
                        }
                    }
                }
            }
            combined
        };

        if !entities.is_empty() {
            total_pii += entities.len();
            for entity in &entities {
                *pii_type_counts.entry(entity.entity_type.to_string()).or_insert(0) += 1;
            }
            let (pseudonymized_text, _records) =
                pseudonymize_text(&field.text, &entities, &state.mapping, &generator);
            replacements.push((field.path.clone(), pseudonymized_text));
        }
    }

    drop(generator);

    if replacements.is_empty() {
        return Ok(PseudonymizeResult {
            body: body_bytes.to_vec(),
            was_pseudonymized: false,
            pii_count: 0,
            pii_types: Vec::new(),
        });
    }

    tracing::info!(pii_count = total_pii, "PII détectées dans la requête");

    let pii_types: Vec<String> = pii_type_counts
        .into_iter()
        .map(|(t, c)| format!("{}:{}", t, c))
        .collect();

    let new_body = rebuild_body(&body, &replacements);
    let new_body_bytes = serde_json::to_vec(&new_body).map_err(|e| e.to_string())?;

    Ok(PseudonymizeResult {
        body: new_body_bytes,
        was_pseudonymized: true,
        pii_count: total_pii,
        pii_types,
    })
}

/// Builds a response with de-pseudonymization.
async fn build_depseudonymized_response(
    upstream: reqwest::Response,
    provider: router::Provider,
    state: &ProxyState,
) -> Result<Response, ProxyError> {
    let status = upstream.status();
    let headers = upstream.headers().clone();

    let is_sse = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false);

    let mut response_builder = Response::builder().status(status.as_u16());
    for (name, value) in headers.iter() {
        // Do not copy content-length since we modify the body
        if name != header::CONTENT_LENGTH {
            response_builder = response_builder.header(name, value);
        }
    }

    if is_sse {
        // SSE streaming mode with de-pseudonymization via buffer
        let mapping = Arc::clone(&state.mapping);
        let byte_stream = upstream.bytes_stream();

        let max_pseudo_len = mapping
            .all_pseudonyms_sorted()
            .first()
            .map(|(p, _)| p.len())
            .unwrap_or(0)
            .max(50);

        let depseudo_stream = futures_util::stream::unfold(
            (byte_stream, StreamBuffer::new(max_pseudo_len), mapping, provider, false),
            |(mut stream, mut buffer, mapping, provider, mut done)| async move {
                if done {
                    return None;
                }

                match stream.next().await {
                    Some(Ok(chunk)) => {
                        let chunk_str = String::from_utf8_lossy(&chunk);
                        let mut output = String::new();

                        // Process each SSE event in the chunk
                        for event_str in chunk_str.split("\n\n") {
                            if event_str.trim().is_empty() {
                                continue;
                            }

                            if let Some(event) = parse_sse_chunk(event_str, provider) {
                                if event.is_done {
                                    // Flush the remaining buffer
                                    let remaining = buffer.flush_remaining(&mapping);
                                    if !remaining.is_empty() {
                                        // Cannot easily reconstruct an SSE event here
                                        // The remaining is just accumulated text
                                    }
                                    output.push_str(event_str);
                                    output.push_str("\n\n");
                                    done = true;
                                } else if let Some(text_delta) = &event.text_delta {
                                    // Push the text into the buffer
                                    let flushed = buffer.push(text_delta, &mapping);
                                    if !flushed.is_empty() {
                                        // Reconstruct the SSE event with de-pseudonymized text
                                        if let Some(rebuilt) =
                                            rebuild_sse_chunk(&event, &flushed, provider)
                                        {
                                            output.push_str(&rebuilt);
                                        }
                                    }
                                    // If nothing is flushed, wait for the next token
                                } else {
                                    // Event without text (message_start, etc.) -> pass through as-is
                                    output.push_str(event_str);
                                    output.push_str("\n\n");
                                }
                            } else {
                                // Unparseable chunk -> pass through as-is
                                output.push_str(event_str);
                                output.push_str("\n\n");
                            }
                        }

                        if output.is_empty() {
                            // Nothing to flush yet, but we must continue the stream
                            Some((Ok::<Bytes, std::io::Error>(Bytes::new()), (stream, buffer, mapping, provider, done)))
                        } else {
                            Some((Ok(Bytes::from(output)), (stream, buffer, mapping, provider, done)))
                        }
                    }
                    Some(Err(e)) => {
                        let err = std::io::Error::other(e);
                        Some((Err(err), (stream, buffer, mapping, provider, done)))
                    }
                    None => {
                        // Stream ended, flush the buffer
                        let remaining = buffer.flush_remaining(&mapping);
                        if !remaining.is_empty() {
                            // Last piece
                            Some((Ok(Bytes::from(remaining)), (stream, buffer, mapping, provider, true)))
                        } else {
                            None
                        }
                    }
                }
            },
        );

        let body = Body::from_stream(depseudo_stream);
        response_builder
            .body(body)
            .map_err(|e| ProxyError::Http(e.to_string()))
    } else {
        // Non-streaming mode: de-pseudonymize the full response
        let body_bytes = upstream.bytes().await?;
        let body_str = String::from_utf8_lossy(&body_bytes);

        let depseudonymized = depseudonymize_text(&body_str, &state.mapping);

        tracing::debug!(
            original_size = body_bytes.len(),
            depseudo_size = depseudonymized.len(),
            "Réponse dé-pseudonymisée"
        );

        let body = Body::from(depseudonymized);
        response_builder
            .body(body)
            .map_err(|e| ProxyError::Http(e.to_string()))
    }
}

/// Builds a passthrough response (without de-pseudonymization).
async fn build_passthrough_response(upstream: reqwest::Response) -> Result<Response, ProxyError> {
    let status = upstream.status();
    let headers = upstream.headers().clone();

    let is_sse = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false);

    let mut response_builder = Response::builder().status(status.as_u16());
    for (name, value) in headers.iter() {
        response_builder = response_builder.header(name, value);
    }

    if is_sse {
        let byte_stream = upstream.bytes_stream().map(|result| {
            result.map_err(std::io::Error::other)
        });
        let body = Body::from_stream(byte_stream);
        response_builder
            .body(body)
            .map_err(|e| ProxyError::Http(e.to_string()))
    } else {
        let body_bytes = upstream.bytes().await?;
        let body = Body::from(body_bytes);
        response_builder
            .body(body)
            .map_err(|e| ProxyError::Http(e.to_string()))
    }
}

/// Monitoring dashboard HTML page, embedded in the binary.
const DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MirageIA — Dashboard</title>
<style>
  :root {
    --bg: #0f1117;
    --surface: #1a1d27;
    --border: #2a2d3a;
    --text: #e0e0e0;
    --muted: #888;
    --accent: #6c8cff;
    --green: #4ade80;
    --yellow: #facc15;
    --cyan: #22d3ee;
    --red: #f87171;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    font-family: 'SF Mono', 'Cascadia Code', 'Consolas', monospace;
    background: var(--bg);
    color: var(--text);
    min-height: 100vh;
    padding: 20px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--border);
  }
  h1 { font-size: 1.4em; font-weight: 600; }
  h1 span { color: var(--accent); }
  .status {
    display: flex;
    gap: 20px;
    align-items: center;
  }
  .status-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.85em;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--green);
    animation: pulse 2s infinite;
  }
  .dot.off { background: var(--red); animation: none; }
  .dot.pass { background: var(--yellow); }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 12px;
    margin-bottom: 24px;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 16px;
  }
  .card-label { font-size: 0.75em; color: var(--muted); text-transform: uppercase; letter-spacing: 0.5px; }
  .card-value { font-size: 1.8em; font-weight: 700; margin-top: 4px; }
  .card-value.accent { color: var(--accent); }
  .card-value.green { color: var(--green); }
  .card-value.yellow { color: var(--yellow); }
  .card-value.cyan { color: var(--cyan); }
  .events-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }
  .events-header h2 { font-size: 1em; }
  .btn {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 6px 14px;
    border-radius: 6px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8em;
  }
  .btn:hover { border-color: var(--accent); }
  #events {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    max-height: 60vh;
    overflow-y: auto;
  }
  .event-row {
    display: grid;
    grid-template-columns: 80px 30px 50px 90px 1fr 100px;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 0.85em;
    align-items: center;
    animation: fadeIn 0.3s ease;
  }
  .event-row:last-child { border-bottom: none; }
  @keyframes fadeIn { from { opacity: 0; transform: translateY(-4px); } to { opacity: 1; } }
  .dir-req { color: var(--cyan); }
  .dir-res { color: var(--green); }
  .mode-pii { color: var(--accent); font-weight: 600; }
  .mode-pass { color: var(--yellow); }
  .pii-badge {
    background: rgba(250, 204, 21, 0.15);
    color: var(--yellow);
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 0.8em;
    white-space: nowrap;
  }
  .empty-state {
    text-align: center;
    padding: 60px 20px;
    color: var(--muted);
  }
  .empty-state .icon { font-size: 2em; margin-bottom: 8px; }
</style>
</head>
<body>

<header>
  <h1><span>MirageIA</span> Dashboard</h1>
  <div class="status">
    <div class="status-item">
      <div class="dot" id="statusDot"></div>
      <span id="statusText">Connexion...</span>
    </div>
  </div>
</header>

<div class="cards">
  <div class="card">
    <div class="card-label">Requetes</div>
    <div class="card-value accent" id="totalRequests">0</div>
  </div>
  <div class="card">
    <div class="card-label">PII detectees</div>
    <div class="card-value yellow" id="totalPii">0</div>
  </div>
  <div class="card">
    <div class="card-label">Mappings actifs</div>
    <div class="card-value cyan" id="mappings">0</div>
  </div>
  <div class="card">
    <div class="card-label">Mode</div>
    <div class="card-value green" id="mode">-</div>
  </div>
</div>

<div class="events-header">
  <h2>Flux temps reel</h2>
  <button class="btn" onclick="clearEvents()">Effacer</button>
</div>

<div id="events">
  <div class="empty-state" id="emptyState">
    <div class="icon">~</div>
    <div>En attente de requetes...</div>
  </div>
</div>

<script>
let totalRequests = 0;
let totalPii = 0;

// Health check periodique
async function checkHealth() {
  try {
    const r = await fetch('/health');
    const h = await r.json();
    document.getElementById('statusDot').className = 'dot';
    document.getElementById('statusText').textContent = 'Actif';
    document.getElementById('mappings').textContent = h.pii_mappings || 0;
    document.getElementById('mode').textContent = h.passthrough ? 'PASS' : 'PII';
    document.getElementById('mode').className = 'card-value ' + (h.passthrough ? 'yellow' : 'green');
  } catch {
    document.getElementById('statusDot').className = 'dot off';
    document.getElementById('statusText').textContent = 'Deconnecte';
  }
}
checkHealth();
setInterval(checkHealth, 3000);

// Flux SSE
const evtSource = new EventSource('/events');
evtSource.onmessage = function(e) {
  try {
    const evt = JSON.parse(e.data);
    addEvent(evt);
  } catch {}
};
evtSource.onerror = function() {
  document.getElementById('statusDot').className = 'dot off';
  document.getElementById('statusText').textContent = 'SSE deconnecte';
};

function addEvent(evt) {
  const container = document.getElementById('events');
  const empty = document.getElementById('emptyState');
  if (empty) empty.remove();

  if (evt.direction === '\u2192') {
    totalRequests++;
    document.getElementById('totalRequests').textContent = totalRequests;
  }

  if (evt.pii_count > 0) {
    totalPii += evt.pii_count;
    document.getElementById('totalPii').textContent = totalPii;
  }

  const time = new Date(evt.timestamp).toLocaleTimeString('fr-FR');
  const isReq = evt.direction === '\u2192';
  const dirClass = isReq ? 'dir-req' : 'dir-res';
  const modeClass = evt.passthrough ? 'mode-pass' : 'mode-pii';
  const modeText = evt.passthrough ? 'PASS' : 'PII';
  const piiHtml = evt.pii_count > 0
    ? `<span class="pii-badge">${evt.pii_count} PII</span>`
    : '';
  const modelHtml = evt.model ? `<span style="color:var(--accent);font-size:0.85em">${evt.model}</span>` : '';
  const sizeHtml = isReq && evt.body_size > 0
    ? `<span style="color:var(--muted);font-size:0.85em">${evt.body_size > 1024 ? (evt.body_size/1024).toFixed(1)+'KB' : evt.body_size+'B'}</span>`
    : '';
  const statusHtml = !isReq && evt.status_code
    ? `<span style="color:${evt.status_code < 400 ? 'var(--green)' : evt.status_code < 500 ? 'var(--yellow)' : 'var(--red)'}">${evt.status_code}</span>`
    : '';
  const durationHtml = !isReq && evt.duration_ms != null
    ? `<span style="color:var(--muted);font-size:0.85em">${evt.duration_ms}ms</span>`
    : '';
  const streamHtml = !isReq && evt.streaming ? `<span style="color:var(--cyan);font-size:0.85em">streaming</span>` : '';
  const piiTypesHtml = isReq && evt.pii_types && evt.pii_types.length > 0
    ? `<div style="color:var(--yellow);font-size:0.8em;padding-left:2em">\u251c\u2500\u2500 ${evt.pii_types.join(', ')}</div>`
    : '';

  const row = document.createElement('div');
  row.className = 'event-row';
  row.innerHTML = `
    <span>${time}</span>
    <span class="${dirClass}">${evt.direction}</span>
    <span class="${modeClass}">${isReq ? modeText : ''}</span>
    <span>${isReq ? '' : statusHtml}</span>
    <span>${evt.provider}</span>
    <span>${evt.path}</span>
    <span>${modelHtml}</span>
    <span>${sizeHtml}${durationHtml}</span>
    <span>${piiHtml}${streamHtml}</span>
  ` + piiTypesHtml;

  container.insertBefore(row, container.firstChild);

  // Limiter a 200 lignes
  while (container.children.length > 200) {
    container.removeChild(container.lastChild);
  }
}

function clearEvents() {
  const container = document.getElementById('events');
  container.innerHTML = '<div class="empty-state" id="emptyState"><div class="icon">~</div><div>En attente de requetes...</div></div>';
  totalRequests = 0;
  totalPii = 0;
  document.getElementById('totalRequests').textContent = '0';
  document.getElementById('totalPii').textContent = '0';
}
</script>
</body>
</html>"##;
