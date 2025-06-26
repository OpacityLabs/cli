use crate::commands::bundle::bundle;

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

use anyhow::Result;
use std::path::Path;

use notify::event::{DataChange, EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;



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

    let script_path =
        PathBuf::from(config.settings.output_directory).join(format!("{}.bundle.lua", name));
    let script_content =
        fs::read_to_string(script_path).map_err(|_| String::from("Script file not found"))?;

    Ok(FlowResponse {
        name: matched_flow.alias.clone(),
        min_sdk: match &matched_flow.min_sdk_version {
            None => {
                info!(
                    "No min SDK version found for flow {}; Defaulting to '1'",
                    name
                );
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
                "Script file not found" => {
                    (axum::http::StatusCode::NOT_FOUND, "Script file not found")
                }
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

async fn watch(config_path: &str) -> notify::Result<()> {
    let (tx, mut rx) = mpsc::channel::<Event>(100);

    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            let _ = tx.try_send(event);
        } else if let Err(e) = res {
            eprintln!("Watch error: {:?}", e);
        }
    })?;

    watcher.watch(Path::new("src"), RecursiveMode::Recursive)?;
    watcher.watch(Path::new(config_path), RecursiveMode::NonRecursive)?;
    info!("Watching all files in 'src' and '{}'", config_path);

    while let Some(_event) = rx.recv().await {
        if _event.kind == EventKind::Modify(ModifyKind::Data(DataChange::Content)) {
            if let Err(err) = bundle(config_path, true) {
                tracing::error!("🟥 Rebundle failed: {:?}", err)
            }
        }
    }

    Ok(())
}

pub async fn serve(config_path: &str, should_watch: &bool) -> Result<(), Box<dyn std::error::Error>> {
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

    if *should_watch {
        tokio::try_join!(
            async {
                let listener = tokio::net::TcpListener::bind(addr).await?;
                axum::serve(listener, app.into_make_service()).await?;
                Ok::<_, Box<dyn std::error::Error>>(())
            },
            async {
                watch(config_path).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            }
        )?;
    } else {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app.into_make_service()).await?;
    }

    Ok(())
}
