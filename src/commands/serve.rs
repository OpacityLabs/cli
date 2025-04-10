use axum::{
    extract::Query,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{fs, net::SocketAddr, path::PathBuf};
use tower::ServiceBuilder;
use tower_http::trace::{self, TraceLayer};
use tracing::{info, Level};
use uuid::Uuid;

#[derive(Deserialize)]
struct FlowQuery {
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowResponse {
    name: String,
    min_sdk: String,
    script: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    id: String,
    created_at: String,
    api_key_id: String,
}

async fn read_flow(name: &str) -> Result<FlowResponse, String> {

    let config = crate::config::Config::from_file("./opacity.toml").unwrap();

    let matched_flow = config
        .platforms
        .iter()
        .flat_map(|platform| platform.flows.iter())
        .find(|flow| flow.alias == name)
        .ok_or_else(|| String::from("Flow not found"))?;

    let script_path = PathBuf::from("./bundled").join(format!("{}.bundle.lua", name));
    let script_content =
        fs::read_to_string(script_path).map_err(|_| String::from("Script file not found"))?;

    Ok(FlowResponse {
        name: matched_flow.alias.clone(),
        min_sdk: match &matched_flow.min_sdk_version {
            None => {
                info!("No min SDK version found for flow {}; Defaulting to '1'", name);
                "1".to_string()
            }
            Some(min_sdk) => min_sdk.clone(),
        },
        script: script_content,
    })
}

async fn flows(Query(query): Query<FlowQuery>) -> Response {
    match read_flow(&query.name).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            let (status, message) = match e.as_str() {
                "Flow not found" => (axum::http::StatusCode::NOT_FOUND, "Flow not found"),
                "Script file not found" => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Script file not found",
                ),
                _ => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Error processing flow request",
                ),
            };
            (status, Json(serde_json::json!({ "error": message }))).into_response()
        }
    }
}

async fn health() -> &'static str {
    "healthy"
}

async fn sessions() -> Json<SessionResponse> {
    Json(SessionResponse {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        api_key_id: "secret-1234".to_string(),
    })
}

pub async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let port = 8080;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let middleware = ServiceBuilder::new().layer(
        TraceLayer::new_for_http()
            .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
            .on_response(
                |response: &axum::response::Response,
                 latency: std::time::Duration,
                 _span: &tracing::Span| {
                    let status = response.status().as_u16();
                    let symbol = if status >= 400 { "🟥" } else { "🟩" };
                    info!("{} {} ({}ms)", symbol, status, latency.as_millis());
                },
            ),
    );

    let app = Router::new()
        .route("/health", get(health))
        .route("/v2/flows", get(flows))
        .route("/sessions", post(sessions))
        .layer(middleware);

    info!("Listening on port {}...", port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
