use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use axum::response::IntoResponse;
use axum::Router;
use tokio::net::TcpListener;

/// Lance un serveur mock qui capture la requête et fait écho au contenu.
async fn start_mock_upstream() -> (SocketAddr, Arc<tokio::sync::Mutex<Option<serde_json::Value>>>) {
    let captured = Arc::new(tokio::sync::Mutex::new(None::<serde_json::Value>));
    let captured_clone = captured.clone();

    let app = Router::new().fallback(move |request: Request<Body>| {
        let cap = captured_clone.clone();
        async move {
            let body_bytes = axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024)
                .await
                .unwrap();

            let body_json: serde_json::Value =
                serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);

            *cap.lock().await = Some(body_json.clone());

            // Réponse Anthropic-like qui fait écho au contenu du premier message
            let echo_text = body_json
                .get("messages")
                .and_then(|m| m.get(0))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("(vide)")
                .to_string();

            let response = serde_json::json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": format!("Réponse avec écho: {}", echo_text)
                }]
            });

            (
                axum::http::StatusCode::OK,
                [("content-type", "application/json")],
                serde_json::to_string(&response).unwrap(),
            )
                .into_response()
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Laisser le serveur démarrer
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    (addr, captured)
}

/// Lance le proxy MirageIA pointant vers le mock upstream.
async fn start_proxy(upstream_addr: SocketAddr) -> SocketAddr {
    start_proxy_with_config(upstream_addr, false).await
}

/// Lance le proxy MirageIA avec option passthrough.
async fn start_proxy_with_config(upstream_addr: SocketAddr, passthrough: bool) -> SocketAddr {
    let mut config = mirageia::config::AppConfig::default();
    config.listen_addr = "127.0.0.1:0".to_string();
    config.anthropic_base_url = format!("http://{}", upstream_addr);
    config.passthrough = passthrough;

    let state = Arc::new(mirageia::proxy::create_state(config));
    let app = mirageia::proxy::create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    addr
}

#[tokio::test]
async fn test_health_endpoint() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/health", proxy_addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_pseudonymize_email_in_request() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Contactez jean.dupont@acme.fr pour le projet"
        }]
    });

    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // La requête envoyée au mock ne doit PAS contenir l'email original
    let captured_body = captured.lock().await;
    let captured_body = captured_body.as_ref().expect("Requête non capturée");

    let sent_content = captured_body["messages"][0]["content"]
        .as_str()
        .unwrap();

    assert!(
        !sent_content.contains("jean.dupont@acme.fr"),
        "L'email original NE DOIT PAS être dans la requête upstream. Reçu: {}",
        sent_content
    );
    assert!(
        sent_content.contains("@example.com"),
        "Un email pseudonymisé DOIT être présent. Reçu: {}",
        sent_content
    );
}

#[tokio::test]
async fn test_depseudonymize_email_in_response() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "L'email est alice@corp.com, merci"
        }]
    });

    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // La réponse (qui fait écho au contenu pseudonymisé) doit être dé-pseudonymisée
    let response_text = resp.text().await.unwrap();

    assert!(
        response_text.contains("alice@corp.com"),
        "L'email original DOIT être restauré dans la réponse. Reçu: {}",
        response_text
    );
}

#[tokio::test]
async fn test_depseudonymize_ip_in_response() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "L'IP du serveur est 192.168.1.50 merci"
        }]
    });

    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let response_text = resp.text().await.unwrap();

    assert!(
        response_text.contains("192.168.1.50"),
        "L'IP originale DOIT être restaurée dans la réponse. Reçu: {}",
        response_text
    );
}

#[tokio::test]
async fn test_no_pii_passthrough() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Bonjour, comment allez-vous ?"
        }]
    });

    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Sans PII, la requête passe telle quelle
    let captured_body = captured.lock().await;
    let captured_body = captured_body.as_ref().unwrap();

    assert_eq!(
        captured_body["messages"][0]["content"],
        "Bonjour, comment allez-vous ?"
    );
}

#[tokio::test]
async fn test_multiple_pii_pseudonymized() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Email: alice@corp.com, IP: 172.16.0.1, Tel: 06 11 22 33 44"
        }]
    });

    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let captured_body = captured.lock().await;
    let sent = captured_body.as_ref().unwrap()["messages"][0]["content"]
        .as_str()
        .unwrap();

    assert!(!sent.contains("alice@corp.com"), "Email non pseudonymisé: {}", sent);
    assert!(!sent.contains("172.16.0.1"), "IP non pseudonymisée: {}", sent);
    assert!(!sent.contains("06 11 22 33 44"), "Téléphone non pseudonymisé: {}", sent);
}

#[tokio::test]
async fn test_unknown_path_returns_404() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/unknown/path", proxy_addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

// ─── Tests mode passthrough ───────────────────────────────────

#[tokio::test]
async fn test_passthrough_mode_no_pseudonymization() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy_with_config(upstream_addr, true).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Email: alice@corp.com, IP: 172.16.0.1"
        }]
    });

    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // En mode passthrough, les PII ne sont PAS pseudonymisées
    let captured_body = captured.lock().await;
    let sent = captured_body.as_ref().unwrap()["messages"][0]["content"]
        .as_str()
        .unwrap();

    assert!(
        sent.contains("alice@corp.com"),
        "En passthrough, l'email DOIT rester tel quel. Reçu: {}",
        sent
    );
    assert!(
        sent.contains("172.16.0.1"),
        "En passthrough, l'IP DOIT rester telle quelle. Reçu: {}",
        sent
    );
}

#[tokio::test]
async fn test_health_shows_passthrough_status() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy_with_config(upstream_addr, true).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/health", proxy_addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["passthrough"], true);
}

// ─── Tests endpoint /events (console) ─────────────────────────

#[tokio::test]
async fn test_events_endpoint_returns_sse() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/events", proxy_addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/event-stream"),
        "Le endpoint /events doit retourner du SSE"
    );
}

#[tokio::test]
async fn test_events_emitted_on_request() {
    let (upstream_addr, _) = start_mock_upstream().await;

    // Créer le proxy manuellement pour accéder au events_tx
    let mut config = mirageia::config::AppConfig::default();
    config.listen_addr = "127.0.0.1:0".to_string();
    config.anthropic_base_url = format!("http://{}", upstream_addr);

    let state = Arc::new(mirageia::proxy::create_state(config));
    let mut events_rx = state.events_tx.subscribe();
    let app = mirageia::proxy::create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Envoyer une requête avec PII
    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Email: test@example.org"
        }]
    });

    client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    // Vérifier qu'on reçoit des événements (requête + réponse)
    let event1 = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events_rx.recv(),
    )
    .await
    .expect("Timeout en attendant l'événement")
    .expect("Erreur de réception");

    assert_eq!(event1.provider, "Anthropic");
    assert_eq!(event1.path, "/v1/messages");
    assert!(!event1.passthrough);

    let event2 = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events_rx.recv(),
    )
    .await
    .expect("Timeout en attendant l'événement réponse")
    .expect("Erreur de réception");

    assert_eq!(event2.provider, "Anthropic");
}

// ─── Tests dashboard web ──────────────────────────────────────

#[tokio::test]
async fn test_dashboard_returns_html() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/dashboard", proxy_addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("text/html"), "Le dashboard doit retourner du HTML");

    let body = resp.text().await.unwrap();
    assert!(body.contains("MirageIA"), "Le dashboard doit contenir le titre");
    assert!(body.contains("EventSource"), "Le dashboard doit se connecter au SSE");
}

// ─── Tests arrêt gracieux ────────────────────────────────────

#[tokio::test]
async fn test_shutdown_endpoint() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();

    // Le proxy doit répondre au health
    let resp = client
        .get(format!("http://{}/health", proxy_addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Envoyer le signal d'arrêt
    let resp = client
        .post(format!("http://{}/shutdown", proxy_addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Laisser le temps au serveur de s'arrêter
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Le proxy ne doit plus répondre
    let result = client
        .get(format!("http://{}/health", proxy_addr))
        .send()
        .await;
    assert!(result.is_err(), "Le proxy devrait être arrêté");
}

#[tokio::test]
async fn test_shutdown_rejects_get() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    // GET sur /shutdown ne doit pas arrêter le proxy
    let result = client
        .get(format!("http://{}/shutdown", proxy_addr))
        .send()
        .await;

    // Devrait retourner 404 (unknown provider) et le proxy tourne encore
    let resp = client
        .get(format!("http://{}/health", proxy_addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "Le proxy doit encore tourner après un GET /shutdown");
    drop(result);
}

// ─── Tests événements enrichis ───────────────────────────────

#[tokio::test]
async fn test_events_contain_enriched_fields() {
    let (upstream_addr, _) = start_mock_upstream().await;

    let mut config = mirageia::config::AppConfig::default();
    config.listen_addr = "127.0.0.1:0".to_string();
    config.anthropic_base_url = format!("http://{}", upstream_addr);

    let state = Arc::new(mirageia::proxy::create_state(config));
    let mut events_rx = state.events_tx.subscribe();
    let app = mirageia::proxy::create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Contactez alice@corp.com et 192.168.1.50"
        }]
    });

    client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    // Événement requête
    let req_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events_rx.recv(),
    )
    .await
    .expect("Timeout événement requête")
    .expect("Erreur réception");

    assert_eq!(req_event.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert!(req_event.body_size > 0, "body_size doit être > 0");
    assert!(req_event.pii_count >= 2, "Au moins 2 PII (email + IP)");
    assert!(!req_event.pii_types.is_empty(), "pii_types ne doit pas être vide");
    assert!(req_event.status_code.is_none(), "Pas de status_code sur la requête");

    // Événement réponse
    let resp_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events_rx.recv(),
    )
    .await
    .expect("Timeout événement réponse")
    .expect("Erreur réception");

    assert!(resp_event.status_code.is_some(), "status_code doit être présent sur la réponse");
    assert_eq!(resp_event.status_code.unwrap(), 200);
    assert!(resp_event.duration_ms.is_some(), "duration_ms doit être présent");
    assert!(resp_event.streaming.is_some(), "streaming doit être présent");
}

#[tokio::test]
async fn test_events_passthrough_contain_enriched_fields() {
    let (upstream_addr, _) = start_mock_upstream().await;

    let mut config = mirageia::config::AppConfig::default();
    config.listen_addr = "127.0.0.1:0".to_string();
    config.anthropic_base_url = format!("http://{}", upstream_addr);
    config.passthrough = true;

    let state = Arc::new(mirageia::proxy::create_state(config));
    let mut events_rx = state.events_tx.subscribe();
    let app = mirageia::proxy::create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Bonjour"
        }]
    });

    client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&request_body)
        .send()
        .await
        .unwrap();

    // Événement requête passthrough
    let req_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events_rx.recv(),
    )
    .await
    .expect("Timeout")
    .expect("Erreur");

    assert!(req_event.passthrough);
    assert_eq!(req_event.model.as_deref(), Some("claude-sonnet-4-20250514"));
    assert!(req_event.body_size > 0);

    // Événement réponse passthrough
    let resp_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events_rx.recv(),
    )
    .await
    .expect("Timeout")
    .expect("Erreur");

    assert!(resp_event.passthrough);
    assert_eq!(resp_event.status_code.unwrap(), 200);
    assert!(resp_event.duration_ms.is_some());
}
