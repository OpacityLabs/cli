use crate::{
    commands::bundle::{create_options, process_bundle},
    config::{Config, Flow, SimplePlatform},
};

use axum::{
    extract::Query,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use darklua_core::Resources;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, net::SocketAddr, path::PathBuf};
use tower::ServiceBuilder;
use tower_http::trace::{self, TraceLayer};
use tracing::{info, Level};
use uuid::Uuid;

use anyhow::Result;

use std::sync::OnceLock;

#[derive(Deserialize)]
struct FlowQuery {
    name: String,
}

#[derive(Deserialize)]
struct FlowQueryV3 {
    alias: String,
}

#[derive(Deserialize)]
struct ResolveMapperAliasQuery {
    alias: String,
}

#[derive(Deserialize)]
struct ResolveMapperFileQuery {
    hash: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LuaScriptOwnerType {
    Custom,
    Opacity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FlowResponse {
    name: String,
    min_sdk: String,
    script: String,
    session_id: String,
    session_action_id: String,
    owner_type: LuaScriptOwnerType,
}

#[derive(Serialize)]
// #[serde(rename_all = "camelCase")]
struct MapperResponse {
    mapper_hash: String,
    mapper_url: String,
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
        PathBuf::from(config.settings.output_directory).join(format!("{}.bundle.luau", name));
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
        session_id: "dummy".to_string(),
        session_action_id: "dummy-action-id".to_string(),
        // the Custom type makes it so NO errors are sent to sentry
        // WARNING! As this is also used by our clients that write
        // their own scripts, we won't be able to see errors in
        // sentry, even if they compile in release mode
        owner_type: LuaScriptOwnerType::Custom,
    })
}

async fn rebundle_and_read_flow(name: &str) -> Result<FlowResponse, String> {
    let config = crate::config::Config::from_file("./opacity.toml").unwrap();

    let (matched_flow, platform_index) = ALIAS_TO_FLOW_MAP_AND_PLATFORM_INDEX
        .get()
        .unwrap()
        .get(name)
        .ok_or("Flow not found")?;
    let flow_platform = &PLATFORM_VECTOR.get().unwrap()[*platform_index];

    if *SHOULD_REBUNDLE.get().unwrap() {
        let bundle_options =
            create_options(&config, flow_platform, matched_flow).map_err(|e| e.to_string())?;

        process_bundle(&Resources::from_file_system(), bundle_options.opts)
            .map_err(|e| e.to_string())?;
    }

    let script_path =
        PathBuf::from(config.settings.output_directory).join(format!("{}.bundle.luau", name));

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
        session_id: "dummy".to_string(),
        session_action_id: "dummy-action-id".to_string(),
        // the Custom type makes it so NO errors are sent to sentry
        // WARNING! As this is also used by our clients that write
        // their own scripts, we won't be able to see errors in
        // sentry, even if they compile in release mode
        owner_type: LuaScriptOwnerType::Custom,
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

// async fn flowsv3(Query(query): Query<FlowQueryV3>) -> Response {
//     match read_flow(&query.alias).await {
//         Ok(response) => Json(response).into_response(),
//         Err(e) => {
//             let (status, message) = match e.as_str() {
//                 "Flow not found" => (axum::http::StatusCode::NOT_FOUND, "Flow not found"),
//                 "Script file not found" => {
//                     (axum::http::StatusCode::NOT_FOUND, "Script file not found")
//                 }
//                 _ => (
//                     axum::http::StatusCode::INTERNAL_SERVER_ERROR,
//                     "Error processing flow request",
//                 ),
//             };
//             (status, Json(serde_json::json!({ "error": message }))).into_response()
//         }
//     }
// }

static ALIAS_TO_PLATFORM_INDEX_MAP: OnceLock<HashMap<String, usize>> = OnceLock::new();
static PLATFORM_VECTOR: OnceLock<Vec<SimplePlatform>> = OnceLock::new();
static ALIAS_TO_FLOW_MAP_AND_PLATFORM_INDEX: OnceLock<HashMap<String, (Flow, usize)>> =
    OnceLock::new();

static MAPPERS_FOLDER: OnceLock<String> = OnceLock::new();
static MAPPERS_CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get_platform_vector(config: &Config) -> &Vec<SimplePlatform> {
    PLATFORM_VECTOR.get_or_init(|| config.platforms.iter().map(SimplePlatform::from).collect())
}

pub fn get_alias_to_platform_index_map(config: &Config) -> &HashMap<String, usize> {
    ALIAS_TO_PLATFORM_INDEX_MAP.get_or_init(|| {
        let platform_vector = get_platform_vector(config);
        let mut hashmap = HashMap::with_capacity(platform_vector.len());

        for (index, platform) in platform_vector.iter().enumerate() {
            hashmap.insert(platform.name.clone(), index);
        }

        hashmap
    })
}

pub fn get_alias_to_flow_map_and_platform_index(
    config: &Config,
) -> &HashMap<String, (Flow, usize)> {
    ALIAS_TO_FLOW_MAP_AND_PLATFORM_INDEX.get_or_init(|| {
        let alias_to_platform_index_map = get_alias_to_platform_index_map(config);
        let mut hashmap = HashMap::new();

        for platform in config.platforms.iter() {
            for flow in platform.flows.iter() {
                hashmap.insert(
                    flow.alias.clone(),
                    (
                        flow.clone(),
                        *alias_to_platform_index_map
                            .get(&platform.name.clone())
                            .unwrap(),
                    ),
                );
            }
        }

        hashmap
    })
}

async fn flowsv3_v2(Query(query): Query<FlowQueryV3>) -> Response {
    match rebundle_and_read_flow(&query.alias).await {
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

static SHOULD_REBUNDLE: OnceLock<bool> = OnceLock::new();

static MAPPER_TO_HASH: OnceLock<HashMap<String, String>> = OnceLock::new();
static HASH_TO_MAPPER: OnceLock<HashMap<String, String>> = OnceLock::new();

async fn resolve_mapper_file(
    headers: axum::http::HeaderMap,
    Query(query): Query<ResolveMapperFileQuery>,
) -> Response {
    let wants_binary = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/octet-stream"))
        .unwrap_or(false);

    match HASH_TO_MAPPER.get() {
        None => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Hash to mapper map not initialized",
            )
                .into_response();
        }
        Some(hash_to_mapper) => match hash_to_mapper.get(&query.hash) {
            None => {
                return (axum::http::StatusCode::NOT_FOUND, "Mapper not found").into_response();
            }
            Some(mapper) => {
                // now we have to get the actual alias back and return the file content
                match MAPPERS_FOLDER.get() {
                    None => {
                        return (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            "Mappers folder not initialized",
                        )
                            .into_response();
                    }
                    Some(mappers_folder) => match MAPPERS_CONFIG.get() {
                        None => {
                            return (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                "Mapper config not initialized",
                            )
                                .into_response();
                        }
                        Some(mappers_config) => {
                            let file_path = PathBuf::from(mappers_folder)
                                .join(mappers_config.settings.output_directory.clone())
                                .join(format!("{}.bundle.luau", mapper));

                            if wants_binary {
                                let file_bytes =
                                    fs::read(&file_path).map_err(|e| e.to_string()).unwrap();
                                return (
                                    [(
                                        axum::http::header::CONTENT_TYPE,
                                        "application/octet-stream",
                                    )],
                                    file_bytes,
                                )
                                    .into_response();
                            } else {
                                let file_content = fs::read_to_string(file_path)
                                    .map_err(|e| e.to_string())
                                    .unwrap();
                                return Json(file_content).into_response();
                            }
                        }
                    },
                }
            }
        },
    }
}
async fn resolve_mapper_alias(Query(query): Query<ResolveMapperAliasQuery>) -> Response {
    match MAPPER_TO_HASH.get() {
        None => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Mapper to hash map not initialized",
            )
                .into_response();
        }
        Some(mapper_to_hash) => match mapper_to_hash.get(&query.alias) {
            None => {
                return (axum::http::StatusCode::NOT_FOUND, "Mapper not found").into_response();
            }
            Some(hash) => {
                return Json(MapperResponse {
                    mapper_hash: hash.clone(),
                    // http://localhost:8080/resolve_mapper_file?hash=...
                    mapper_url: format!("http://localhost:8080/resolve_mapper_file?hash={}", hash),
                })
                .into_response();
            }
        },
    }
}

fn initialize_mappers_folder_config_and_hashmaps(
    mappers_location: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // try to read the file
    let file_content = fs::read_to_string(mappers_location).map_err(|e| e.to_string())?;

    // first line of the mappers file
    let mappers_folder = file_content
        .lines()
        .next()
        .ok_or("No mappers folder found")?
        .to_string();

    MAPPERS_FOLDER.set(mappers_folder.clone()).unwrap(); // should be initialized only once
    MAPPERS_CONFIG
        .set(
            Config::from_file(
                PathBuf::from(mappers_folder.clone())
                    .join("opacity.toml")
                    .to_str()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap(); // should be initialized only once

    // initialize the hashmaps
    let mut mapper_to_hash = HashMap::new();
    let mut hash_to_mapper = HashMap::new();

    // so now we have to set the hashmaps
    // we first need to read `mapper-index.json` from the mappers folder
    let mapper_index_path = PathBuf::from(mappers_folder).join("mapper-index.json");
    let mapper_index_content = fs::read_to_string(mapper_index_path).map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct MapperValue {
        luau_hash: String,
        #[allow(dead_code)]
        schema_hash: String,
    }

    let mapper_index: HashMap<String, MapperValue> = serde_json::from_str(&mapper_index_content)?;

    for (mapper, value) in mapper_index.iter() {
        mapper_to_hash.insert(mapper.clone(), value.luau_hash.clone());
        hash_to_mapper.insert(value.luau_hash.clone(), mapper.clone());
    }

    MAPPER_TO_HASH.set(mapper_to_hash).unwrap(); // should be initialized only once
    HASH_TO_MAPPER.set(hash_to_mapper).unwrap(); // should be initialized only once

    Ok(())
}

pub async fn serve(
    config_path: &str,
    should_rebundle: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // initialize everything
    get_alias_to_flow_map_and_platform_index(&Config::from_file(config_path).unwrap());
    SHOULD_REBUNDLE.get_or_init(|| should_rebundle);

    match initialize_mappers_folder_config_and_hashmaps("./mappers.location") {
        Ok(_) => {
            info!(
                "Mappers folder and config initialized, serving mappers from folder: {}",
                MAPPERS_FOLDER.get().unwrap()
            );
        }
        // if there is no mappers.location file, we are not going to serve mappers
        Err(e) => match e.to_string().as_str() {
            "File not found" => {
                info!("No mappers.location file found, not serving mappers");
            }
            "No mappers folder found" => {
                info!("No mappers folder found, not serving mappers");
            }
            _ => {
                return Err(e);
            }
        },
    }

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
        .route("/v3/flows", get(flowsv3_v2))
        .route("/sessions", post(sessions))
        .route("/resolve_mapper_alias", get(resolve_mapper_alias)) // this returns { mapper_hash: String, mapper_url: String }
        .route("/resolve_mapper_file", get(resolve_mapper_file)) // this returns the file content
        .layer(middleware);

    info!(
        "Listening on port {} (with rebundle {}...)...",
        port,
        if should_rebundle {
            "enabled"
        } else {
            "disabled"
        }
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
