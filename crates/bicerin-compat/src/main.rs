use anyhow::{bail, Context, Result};
use clap::Parser;
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Parser)]
#[command(name = "bicerin-compat", about = "Run Bicerin compatibility probes against a live homeserver")]
struct Args {
    #[arg(long, env = "BICERIN_COMPAT_BASE_URL")]
    base_url: String,
    #[arg(long, env = "BICERIN_COMPAT_ACCESS_TOKEN")]
    access_token: Option<String>,
    /// Comma-separated suites: client-api,sync,media,e2ee,push,websocket,appservice.
    #[arg(long, value_delimiter = ',', default_value = "client-api,sync,media,e2ee,push,websocket")]
    suites: Vec<String>,
    #[arg(long, env = "BICERIN_COMPAT_APPSERVICE_ID")]
    appservice_id: Option<String>,
    #[arg(long, env = "BICERIN_COMPAT_APPSERVICE_TOKEN")]
    appservice_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let client = Client::new();
    let mut failures = Vec::new();

    for suite in &args.suites {
        let result = match suite.as_str() {
            "client-api" => client_api(&client, &args).await,
            "sync" => authenticated_json(&client, &args, "/_matrix/client/v3/sync?timeout=0").await,
            "media" => unauthenticated_json(&client, &args, "/_matrix/media/v3/config").await,
            "e2ee" => authenticated_json(&client, &args, "/_matrix/client/v3/keys/changes?from=s0&to=s0").await,
            "push" => authenticated_json(&client, &args, "/_matrix/client/v3/pushrules/").await,
            "websocket" => websocket(&args).await,
            "appservice" => appservice(&client, &args).await,
            other => bail!("unknown compatibility suite: {other}"),
        };

        match result {
            Ok(()) => println!("PASS {suite}"),
            Err(error) if suite == "appservice" && error.to_string() == "not configured" => {
                println!("SKIP appservice (BICERIN_COMPAT_APPSERVICE_ID/TOKEN not configured)")
            }
            Err(error) => {
                println!("FAIL {suite}: {error:#}");
                failures.push(suite.clone());
            }
        }
    }

    if !failures.is_empty() {
        bail!("compatibility suites failed: {}", failures.join(", "));
    }
    Ok(())
}

async fn client_api(client: &Client, args: &Args) -> Result<()> {
    unauthenticated_json(client, args, "/_matrix/client/versions").await?;
    authenticated_json(client, args, "/_matrix/client/v3/account/whoami").await
}

async fn authenticated_json(client: &Client, args: &Args, path: &str) -> Result<()> {
    let token = args.access_token.as_deref().context("access token is required for this suite")?;
    let response = client
        .get(endpoint(args, path))
        .bearer_auth(token)
        .send()
        .await
        .context("request failed")?;
    ensure_json_success(response).await
}

async fn unauthenticated_json(client: &Client, args: &Args, path: &str) -> Result<()> {
    let response = client.get(endpoint(args, path)).send().await.context("request failed")?;
    ensure_json_success(response).await
}

async fn appservice(client: &Client, args: &Args) -> Result<()> {
    let (Some(id), Some(token)) = (&args.appservice_id, &args.appservice_token) else {
        bail!("not configured");
    };
    let response = client
        .post(endpoint(args, &format!("/_matrix/client/v1/appservice/{id}/ping")))
        .bearer_auth(token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .context("appservice ping request failed")?;
    ensure_json_success(response).await
}

async fn websocket(args: &Args) -> Result<()> {
    let token = args.access_token.as_deref().context("access token is required for this suite")?;
    let mut url = url::Url::parse(&endpoint(args, "/_bicerin/ws"))?;
    match url.scheme() {
        "http" => { url.set_scheme("ws").expect("valid websocket scheme"); }
        "https" => { url.set_scheme("wss").expect("valid websocket scheme"); }
        scheme => bail!("unsupported base URL scheme: {scheme}"),
    }
    url.query_pairs_mut().append_pair("access_token", token);
    let (mut socket, _) = tokio_tungstenite::connect_async(url.as_str()).await.context("websocket upgrade failed")?;
    let message = socket.next().await.context("server closed WebSocket before initial sync")??;
    let Message::Text(payload) = message else {
        bail!("server did not send an initial JSON sync response");
    };
    let response: serde_json::Value = serde_json::from_str(&payload).context("initial WebSocket payload is not JSON")?;
    if response.get("next_batch").is_none() {
        bail!("initial WebSocket response is not a Matrix sync response");
    }
    let _ = socket.close(None).await;
    Ok(())
}

async fn ensure_json_success(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let body = response.text().await.context("failed to read response body")?;
    if status != StatusCode::OK {
        bail!("expected HTTP 200, got {status}: {body}");
    }
    serde_json::from_str::<serde_json::Value>(&body).context("response was not JSON")?;
    Ok(())
}

fn endpoint(args: &Args, path: &str) -> String {
    format!("{}{}", args.base_url.trim_end_matches('/'), path)
}

#[cfg(test)]
mod tests {
    use super::{endpoint, Args};

    #[test]
    fn joins_base_url_and_matrix_path_once() {
        let args = Args { base_url: "https://matrix.example/".into(), access_token: None, suites: vec![], appservice_id: None, appservice_token: None };
        assert_eq!(endpoint(&args, "/_matrix/client/versions"), "https://matrix.example/_matrix/client/versions");
    }
}