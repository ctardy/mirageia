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
    let mut config = mirageia::config::AppConfig::default();
    config.listen_addr = "127.0.0.1:0".to_string();
    config.anthropic_base_url = format!("http://{}", upstream_addr);

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
