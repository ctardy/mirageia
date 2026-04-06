use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
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

    let mut config = mirageia::config::AppConfig::default();
    config.listen_addr = "127.0.0.1:0".to_string();
    config.anthropic_base_url = format!("http://{}", upstream_addr);

    let state = Arc::new(mirageia::proxy::create_state(config));
    let mut shutdown_rx = state.shutdown_tx.subscribe();
    let app = mirageia::proxy::create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.wait_for(|&v| v).await;
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

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
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Le proxy ne doit plus répondre
    let result = client
        .get(format!("http://{}/health", proxy_addr))
        .send()
        .await;
    assert!(result.is_err(), "Le proxy devrait être arrêté");
}

// ─── Cahier de recette — PII types ──────────────────────────

/// RC-01 : Téléphone français pseudonymisé dans la requête upstream
#[tokio::test]
async fn test_pseudonymize_phone_in_request() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Appelez le 06 12 34 56 78 pour confirmer."}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let captured_body = captured.lock().await;
    let sent = captured_body.as_ref().unwrap()["messages"][0]["content"].as_str().unwrap();
    assert!(
        !sent.contains("06 12 34 56 78"),
        "RC-01 FAIL : téléphone non pseudonymisé dans upstream. Reçu: {}",
        sent
    );
}

/// RC-02 : IBAN français pseudonymisé (MOD-97 valide requis pour détection)
#[tokio::test]
async fn test_pseudonymize_iban_in_request() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Virement sur IBAN FR7630006000011234567890189 svp."}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let captured_body = captured.lock().await;
    let sent = captured_body.as_ref().unwrap()["messages"][0]["content"].as_str().unwrap();
    assert!(
        !sent.contains("FR7630006000011234567890189"),
        "RC-02 FAIL : IBAN non pseudonymisé dans upstream. Reçu: {}",
        sent
    );
}

/// RC-02b : IBAN restauré dans la réponse (dé-pseudonymisation)
#[tokio::test]
async fn test_depseudonymize_iban_in_response() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "IBAN du compte : FR7630006000011234567890189"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let response_text = resp.text().await.unwrap();
    assert!(
        response_text.contains("FR7630006000011234567890189"),
        "RC-02b FAIL : IBAN original non restauré dans la réponse. Reçu: {}",
        response_text
    );
}

/// RC-03 : Carte bancaire pseudonymisée (Luhn valide requis)
#[tokio::test]
async fn test_pseudonymize_credit_card_in_request() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Ma carte Visa est 4111111111111111, exp 12/26."}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let captured_body = captured.lock().await;
    let sent = captured_body.as_ref().unwrap()["messages"][0]["content"].as_str().unwrap();
    assert!(
        !sent.contains("4111111111111111"),
        "RC-03 FAIL : numéro CB non pseudonymisé dans upstream. Reçu: {}",
        sent
    );
}

/// RC-04 : Clé API Anthropic pseudonymisée (régression v0.4.3 — chevauchement phone/apikey)
#[tokio::test]
async fn test_pseudonymize_api_key_in_request() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let api_key = "sk-ant-api03-AAABBBCCC0123456789xyzXYZ0123456789abcdefABCDEF-end";
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": format!("Ma clé API est {}", api_key)}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let captured_body = captured.lock().await;
    let sent = captured_body.as_ref().unwrap()["messages"][0]["content"].as_str().unwrap();
    assert!(
        !sent.contains(api_key),
        "RC-04 FAIL : clé API non pseudonymisée dans upstream. Reçu: {}",
        sent
    );
}

/// Construit un PDF minimal avec du texte (dupliqué ici car cfg(test) n'est pas visible depuis les tests d'intégration)
fn build_test_pdf(text_content: &str) -> Vec<u8> {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.into()]),
            Operation::new("Td", vec![100.into(), 700.into()]),
            Operation::new("Tj", vec![Object::string_literal(text_content)]),
            Operation::new("ET", vec![]),
        ],
    };
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut buf = Vec::new();
    doc.save_to(&mut buf).unwrap();
    buf
}

/// Construit un DOCX minimal avec du texte
fn build_test_docx_inline(text: &str) -> Vec<u8> {
    use std::io::{Cursor, Write};
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

    let xml = format!(
        r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#,
        text
    );
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options: FileOptions<'_, ()> =
        FileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

/// RC-05 : Extraction PDF — le document est converti en texte et les PII pseudonymisées
#[tokio::test]
async fn test_document_pdf_extracted_and_pseudonymized() {
    let pdf_bytes = build_test_pdf("Client jean.dupont@acme.fr IP 192.168.1.10");
    let data_b64 = BASE64.encode(&pdf_bytes);

    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "document",
                    "source": {
                        "type": "base64",
                        "media_type": "application/pdf",
                        "data": data_b64
                    }
                }]
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let captured_body = captured.lock().await;
    let content = &captured_body.as_ref().unwrap()["messages"][0]["content"];

    // Le bloc document doit être converti en bloc text
    let block = content.get(0).expect("RC-05 FAIL : aucun bloc dans content");
    assert_eq!(
        block["type"], "text",
        "RC-05 FAIL : le bloc document n'a pas été converti en text. Reçu: {}",
        block
    );

    let text = block["text"].as_str().unwrap();
    assert!(
        text.contains("[document PDF]"),
        "RC-05 FAIL : marqueur [document PDF] absent. Reçu: {}",
        text
    );
    // Les PII dans le document doivent être pseudonymisées
    assert!(
        !text.contains("jean.dupont@acme.fr"),
        "RC-05 FAIL : email non pseudonymisé dans le PDF extrait. Reçu: {}",
        text
    );
    assert!(
        !text.contains("192.168.1.10"),
        "RC-05 FAIL : IP non pseudonymisée dans le PDF extrait. Reçu: {}",
        text
    );
}

/// RC-06 : Extraction DOCX — le document est converti en texte et les PII pseudonymisées
#[tokio::test]
async fn test_document_docx_extracted_and_pseudonymized() {
    let docx_bytes = build_test_docx_inline("Contact alice@corp.com au 06 99 88 77 66");
    let data_b64 = BASE64.encode(&docx_bytes);

    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "document",
                    "source": {
                        "type": "base64",
                        "media_type": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                        "data": data_b64
                    }
                }]
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let captured_body = captured.lock().await;
    let content = &captured_body.as_ref().unwrap()["messages"][0]["content"];
    let block = content.get(0).expect("RC-06 FAIL : aucun bloc dans content");

    assert_eq!(
        block["type"], "text",
        "RC-06 FAIL : le bloc document DOCX n'a pas été converti en text. Reçu: {}",
        block
    );
    let text = block["text"].as_str().unwrap();
    assert!(
        text.contains("[document DOCX]"),
        "RC-06 FAIL : marqueur [document DOCX] absent. Reçu: {}",
        text
    );
    assert!(
        !text.contains("alice@corp.com"),
        "RC-06 FAIL : email non pseudonymisé dans le DOCX extrait. Reçu: {}",
        text
    );
}

/// RC-08 : Secret / mot de passe à haute entropie pseudonymisé
#[tokio::test]
async fn test_pseudonymize_secret_password_in_request() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    // Mot de passe fort : longueur ≥ 12, entropie > 3.5, 3+ classes de caractères
    let password = "P@ssw0rd!Secure99";
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": format!("Mon mot de passe est {}", password)}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let captured_body = captured.lock().await;
    let sent = captured_body.as_ref().unwrap()["messages"][0]["content"].as_str().unwrap();
    assert!(
        !sent.contains(password),
        "RC-08 FAIL : mot de passe non pseudonymisé dans upstream. Reçu: {}",
        sent
    );
}

/// RC-07 : pii_count exact dans les événements proxy
#[tokio::test]
async fn test_pii_count_exact_in_events() {
    let (upstream_addr, _) = start_mock_upstream().await;

    let mut config = mirageia::config::AppConfig::default();
    config.listen_addr = "127.0.0.1:0".to_string();
    config.anthropic_base_url = format!("http://{}", upstream_addr);

    let state = Arc::new(mirageia::proxy::create_state(config));
    let mut events_rx = state.events_tx.subscribe();
    let app = mirageia::proxy::create_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Email: a@b.com IP: 10.0.0.1"}]
        }))
        .send()
        .await
        .unwrap();

    let req_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        events_rx.recv(),
    )
    .await
    .expect("RC-07 FAIL : timeout événement")
    .expect("RC-07 FAIL : erreur réception");

    assert!(
        req_event.pii_count >= 2,
        "RC-07 FAIL : pii_count={} attendu >= 2",
        req_event.pii_count
    );
    let types_str = req_event.pii_types.join(",");
    assert!(
        types_str.contains("EMAIL"),
        "RC-07 FAIL : EMAIL absent de pii_types: {}",
        types_str
    );
    assert!(
        types_str.contains("IP_ADDRESS"),
        "RC-07 FAIL : IP_ADDRESS absent de pii_types: {}",
        types_str
    );
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

// ─── Tests décomposition char-par-char (regression) ───────────────────────────

/// Vérifie que lorsque l'upstream retourne une valeur pseudonymisée décomposée
/// caractère par caractère (format JSON `["c","h","a","r",...]`), MirageIA
/// ré-injecte les caractères originaux dans la réponse.
///
/// Scénario testé :
///   - Requête contient un email réel
///   - Mock upstream reçoit le pseudonyme et le renvoie DÉCOMPOSÉ en chars
///   - La réponse client doit contenir les chars de l'email ORIGINAL
#[tokio::test]
async fn test_depseudonymize_char_array_in_response() {
    // Custom mock: captures the pseudonymized value from the request and
    // returns it decomposed character by character in the response.
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

            // Extract the pseudonymized email from the request
            let content = body_json["messages"][0]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();

            *cap.lock().await = Some(body_json);

            // Build a response that decomposes the content char by char (simulates the LLM)
            let char_array: Vec<String> =
                content.chars().map(|c| format!("\"{}\"", c)).collect();
            let char_array_str = char_array.join(",");
            let echo_text = format!("Décomposition: [{}]", char_array_str);

            let response = serde_json::json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": echo_text}]
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
    let upstream_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 500,
        "messages": [{
            "role": "user",
            "content": "Mon email est alice@corp.com"
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

    // The upstream received the pseudonymized email and returned it char by char.
    // MirageIA must replace those chars with the original email chars in the response.
    // The response body is raw JSON, so quotes are JSON-escaped: \"a\",\"l\",...
    // Two separator styles are accepted: compact (",") and pretty (", ").
    let orig_chars: Vec<String> = "alice@corp.com"
        .chars()
        .map(|c| format!("\\\"{}\\\"", c))
        .collect();
    let orig_char_str_compact = orig_chars.join(",");
    let orig_char_str_pretty = orig_chars.join(", ");
    assert!(
        response_text.contains(&orig_char_str_compact) || response_text.contains(&orig_char_str_pretty),
        "Les chars de l'email original doivent être dans la réponse (JSON-échappée compact ou pretty). Reçu: {}",
        response_text
    );

    // The pseudonymized email chars must NOT appear (JSON-escaped form)
    let captured_req = captured.lock().await;
    let pseudo_email = captured_req
        .as_ref()
        .unwrap()["messages"][0]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let pseudo_value = pseudo_email.trim_start_matches("Mon email est ").trim();
    if pseudo_value != "alice@corp.com" {
        let pseudo_chars: Vec<String> =
            pseudo_value.chars().map(|c| format!("\\\"{}\\\"", c)).collect();
        let pseudo_compact = pseudo_chars.join(",");
        let pseudo_pretty = pseudo_chars.join(", ");
        assert!(
            !response_text.contains(&pseudo_compact) && !response_text.contains(&pseudo_pretty),
            "Les chars pseudonymisés ne doivent PAS être dans la réponse. Reçu: {}",
            response_text
        );
    }
}

/// Vérifie que la ré-injection fonctionne pour un numéro de téléphone (qui contient
/// des espaces) dans une réponse non-streaming : le pseudonyme doit être replacé
/// par la valeur originale même si le numéro est coupé entre tokens.
#[tokio::test]
async fn test_depseudonymize_phone_in_response() {
    let (upstream_addr, _) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": "Mon téléphone est +33 6 12 34 56 78 merci"
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
        response_text.contains("+33 6 12 34 56 78"),
        "Le téléphone original doit être restauré dans la réponse. Reçu: {}",
        response_text
    );
}
