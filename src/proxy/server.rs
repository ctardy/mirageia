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
use crate::mapping::MappingTable;
use crate::pseudonymization::generator::PseudonymGenerator;
use crate::pseudonymization::{depseudonymize_text, pseudonymize_text};
use crate::proxy::extractor::{extract_text_fields, rebuild_body, JsonPath};
use crate::streaming::{parse_sse_chunk, rebuild_sse_chunk, StreamBuffer};

use super::client::UpstreamClient;
use super::error::ProxyError;
use super::router;

/// Direction d'un événement proxy (requête ou réponse).
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

/// Événement émis par le proxy pour la console de monitoring.
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

/// Extrait le nom du modèle depuis le body JSON d'une requête LLM.
fn extract_model_from_body(body_bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body_bytes)
        .ok()
        .and_then(|v| v.get("model")?.as_str().map(String::from))
}

pub struct ProxyState {
    pub config: AppConfig,
    pub client: UpstreamClient,
    pub detector: RegexDetector,
    pub mapping: Arc<MappingTable>,
    pub generator: Arc<Mutex<PseudonymGenerator>>,
    pub events_tx: broadcast::Sender<ProxyEvent>,
}

/// Crée un ProxyState à partir d'une config (pour les tests).
pub fn create_state(config: AppConfig) -> ProxyState {
    let (events_tx, _) = broadcast::channel(256);
    ProxyState {
        config,
        client: UpstreamClient::new(),
        detector: RegexDetector::new(),
        mapping: Arc::new(MappingTable::new()),
        generator: Arc::new(Mutex::new(PseudonymGenerator::new())),
        events_tx,
    }
}

/// Retourne le handler comme fallback pour un Router axum (pour les tests).
pub fn create_router(state: Arc<ProxyState>) -> Router {
    Router::new().fallback(proxy_handler).with_state(state)
}

/// Démarre le proxy HTTP sur l'adresse configurée.
pub async fn start_proxy(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(create_state(config.clone()));

    let app = create_router(state);

    let listener = TcpListener::bind(&config.listen_addr).await?;
    if config.passthrough {
        tracing::info!(
            "MirageIA proxy écoute sur {} (MODE PASSTHROUGH — pas de pseudonymisation)",
            config.listen_addr
        );
    } else {
        tracing::info!("MirageIA proxy écoute sur {}", config.listen_addr);
    }

    axum::serve(listener, app).await?;

    Ok(())
}

/// Headers à copier de la requête entrante vers l'upstream.
const FORWARDED_HEADERS: &[&str] = &[
    "content-type",
    "x-api-key",
    "authorization",
    "anthropic-version",
    "anthropic-beta",
    "accept",
    "user-agent",
];

/// Handler catch-all : reçoit toute requête, la route vers le bon provider, relaye la réponse.
async fn proxy_handler(
    State(state): State<Arc<ProxyState>>,
    request: Request<Body>,
) -> Result<Response, ProxyError> {
    let path = request.uri().path().to_string();

    // Health check
    if path == "/health" {
        let stats = serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "passthrough": state.config.passthrough,
            "pii_mappings": state.mapping.len(),
        });
        return Ok((StatusCode::OK, axum::Json(stats)).into_response());
    }

    // Endpoint SSE pour la console de monitoring
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

    // Dashboard web
    if path == "/dashboard" {
        return Response::builder()
            .status(200)
            .header("content-type", "text/html; charset=utf-8")
            .body(Body::from(DASHBOARD_HTML))
            .map_err(|e| ProxyError::Http(e.to_string()));
    }

    // Résoudre le provider
    let provider = router::resolve_provider(&path)
        .ok_or_else(|| ProxyError::UnknownProvider(path.clone()))?;

    let upstream_url = router::upstream_url(provider, &path, &state.config);

    // Copier les headers pertinents
    let mut upstream_headers = http::HeaderMap::new();
    for &name in FORWARDED_HEADERS {
        if let Some(value) = request.headers().get(name) {
            if let Ok(header_name) = http::header::HeaderName::from_bytes(name.as_bytes()) {
                upstream_headers.insert(header_name, value.clone());
            }
        }
    }

    // Lire le body
    let method = request.method().clone();
    let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::Body(e.to_string()))?;

    let model = extract_model_from_body(&body_bytes);
    let request_body_size = body_bytes.len();

    // --- MODE PASSTHROUGH : relayer sans pseudonymiser ---
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

    // --- PSEUDONYMISATION DE LA REQUÊTE (fail-open) ---
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
            tracing::warn!("Pseudonymisation échouée, passthrough : {}", e);
            PseudonymizeResult {
                body: body_bytes.to_vec(),
                was_pseudonymized: false,
                pii_count: 0,
                pii_types: Vec::new(),
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

    // Envoyer à l'upstream
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

    // --- DÉ-PSEUDONYMISATION DE LA RÉPONSE ---
    if was_pseudonymized {
        build_depseudonymized_response(upstream_response, provider, &state).await
    } else {
        build_passthrough_response(upstream_response).await
    }
}

/// Résultat de la pseudonymisation d'une requête.
struct PseudonymizeResult {
    body: Vec<u8>,
    was_pseudonymized: bool,
    pii_count: usize,
    pii_types: Vec<String>,
}

/// Pseudonymise le body de la requête.
fn pseudonymize_request(
    body_bytes: &[u8],
    provider: router::Provider,
    state: &ProxyState,
) -> Result<PseudonymizeResult, String> {
    let body: serde_json::Value =
        serde_json::from_slice(body_bytes).map_err(|e| format!("JSON invalide : {}", e))?;

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

/// Construit une réponse avec dé-pseudonymisation.
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
        // Ne pas copier content-length car on modifie le body
        if name != header::CONTENT_LENGTH {
            response_builder = response_builder.header(name, value);
        }
    }

    if is_sse {
        // Mode streaming SSE avec dé-pseudonymisation via buffer
        let mapping = Arc::clone(&state.mapping);
        let byte_stream = upstream.bytes_stream();

        // Calculer la longueur max des pseudonymes pour le buffer
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

                        // Traiter chaque événement SSE dans le chunk
                        for event_str in chunk_str.split("\n\n") {
                            if event_str.trim().is_empty() {
                                continue;
                            }

                            if let Some(event) = parse_sse_chunk(event_str, provider) {
                                if event.is_done {
                                    // Flush le buffer restant
                                    let remaining = buffer.flush_remaining(&mapping);
                                    if !remaining.is_empty() {
                                        // On ne peut pas reconstruire un SSE event ici facilement
                                        // Le remaining est juste du texte accumulé
                                    }
                                    output.push_str(event_str);
                                    output.push_str("\n\n");
                                    done = true;
                                } else if let Some(text_delta) = &event.text_delta {
                                    // Pousser le texte dans le buffer
                                    let flushed = buffer.push(text_delta, &mapping);
                                    if !flushed.is_empty() {
                                        // Reconstruire l'événement SSE avec le texte dé-pseudonymisé
                                        if let Some(rebuilt) =
                                            rebuild_sse_chunk(&event, &flushed, provider)
                                        {
                                            output.push_str(&rebuilt);
                                        }
                                    }
                                    // Si rien n'est flushed, on attend le prochain token
                                } else {
                                    // Événement sans texte (message_start, etc.) → passer tel quel
                                    output.push_str(event_str);
                                    output.push_str("\n\n");
                                }
                            } else {
                                // Chunk non parsable → passer tel quel
                                output.push_str(event_str);
                                output.push_str("\n\n");
                            }
                        }

                        if output.is_empty() {
                            // Rien à flusher pour l'instant, mais on doit continuer le stream
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
                        // Stream terminé, flush le buffer
                        let remaining = buffer.flush_remaining(&mapping);
                        if !remaining.is_empty() {
                            // Dernier morceau
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
        // Mode non-streaming : dé-pseudonymiser la réponse complète
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

/// Construit une réponse passthrough (sans dé-pseudonymisation).
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

/// Page HTML du dashboard de monitoring, embarquée dans le binaire.
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
