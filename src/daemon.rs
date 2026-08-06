use anyhow::Context;
use serde::Deserialize;

const BASE_URL: &str = "http://localhost";

#[derive(Deserialize)]
struct SessionResponse {
    session_id: String,
}

/// Check if the daemon health endpoint responds successfully.
pub async fn check_health(port: u16) -> bool {
    let url = format!("{}:{}/api/v1/health", BASE_URL, port);
    match reqwest::get(&url).await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Create a new chat session. Returns the session ID.
pub async fn create_session(port: u16) -> anyhow::Result<String> {
    let url = format!("{}:{}/api/v1/sessions", BASE_URL, port);
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .context("Failed to connect to daemon for session creation")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Session creation failed ({}): {}", status, body);
    }

    let session: SessionResponse = resp
        .json()
        .await
        .context("Failed to parse session response")?;

    Ok(session.session_id)
}

/// Send a prompt to an existing session.
/// Returns the SSE streaming response (caller must read the body stream).
pub async fn send_prompt(
    port: u16,
    session_id: &str,
    prompt: &str,
) -> anyhow::Result<reqwest::Response> {
    let url = format!(
        "{}:{}/api/v1/sessions/{}/chat",
        BASE_URL, port, session_id
    );
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&serde_json::json!({"prompt": prompt}))
        .send()
        .await
        .context("Failed to send prompt to daemon")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Prompt request failed ({}): {}", status, body);
    }

    Ok(resp)
}
