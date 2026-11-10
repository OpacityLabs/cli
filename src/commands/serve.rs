use crate::{
    commands::bundle::{create_options, process_bundle},
    config::{Config, Flow, SimplePlatform},
};

use axum::{
    extract::{Path, Query},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use darklua_core::Resources;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::ErrorKind,
    net::SocketAddr,
    path::PathBuf,
    sync::{LazyLock, RwLock},
};
use tower::ServiceBuilder;
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, Level};
use url::Url;
use uuid::Uuid;

use anyhow::{Context, Result};

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
    allowed_hosts: Option<Vec<String>>,
    mapper_hash: Option<String>,
    mapper_url: Option<String>,
}

impl FlowResponse {
    /// `public_url` is this server's base URL as the SDK reached it. It is
    /// saved for the flow, so `/verify` gives the notary the same mapper URL.
    fn new(flow: &Flow, script: String, public_url: &Url) -> Result<Self, String> {
        let (mapper_hash, mapper_url) = flow_mapper(&flow.alias, public_url)?
            .map(|mapper| (mapper.hash, mapper.url))
            .unzip();
        FLOW_PUBLIC_URLS
            .write()
            .unwrap()
            .insert(flow.alias.clone(), public_url.clone());

        Ok(Self {
            name: flow.alias.clone(),
            min_sdk: match &flow.min_sdk_version {
                None => {
                    info!(
                        "No min SDK version found for flow {}; Defaulting to '1'",
                        flow.alias
                    );
                    "1".to_string()
                }
                Some(min_sdk) => min_sdk.clone(),
            },
            script,
            session_id: "dummy".to_string(),
            session_action_id: session_action_id_for(&flow.alias),
            owner_type: LuaScriptOwnerType::Opacity,
            allowed_hosts: flow.allowed_hosts.clone(),
            mapper_hash,
            mapper_url,
        })
    }
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

async fn read_flow(name: &str, public_url: &Url) -> Result<FlowResponse, String> {
    let config = flows_config()?;

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

    FlowResponse::new(matched_flow, script_content, public_url)
}

async fn rebundle_and_read_flow(name: &str, public_url: &Url) -> Result<FlowResponse, String> {
    let config = flows_config()?;

    let (matched_flow, platform_index) = ALIAS_TO_FLOW_MAP_AND_PLATFORM_INDEX
        .get()
        .unwrap()
        .get(name)
        .ok_or("Flow not found")?;
    let flow_platform = &PLATFORM_VECTOR.get().unwrap()[*platform_index];

    if *SHOULD_REBUNDLE.get().unwrap() {
        let bundle_options =
            create_options(&config, flow_platform, matched_flow).map_err(|e| e.to_string())?;

        process_bundle(&Resources::from_file_system(), bundle_options)
            .map_err(|e| e.to_string())?;
    }

    let script_path =
        PathBuf::from(config.settings.output_directory).join(format!("{}.bundle.luau", name));

    let script_content =
        fs::read_to_string(script_path).map_err(|_| String::from("Script file not found"))?;

    FlowResponse::new(matched_flow, script_content, public_url)
}

async fn flows(headers: axum::http::HeaderMap, Query(query): Query<FlowQuery>) -> Response {
    match read_flow(&query.name, &request_public_url(&headers)).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            let (status, message) = match e.as_str() {
                "Flow not found" => (axum::http::StatusCode::NOT_FOUND, "Flow not found"),
                "Script file not found" => {
                    (axum::http::StatusCode::NOT_FOUND, "Script file not found")
                }
                _ => {
                    error!("Could not serve flow {}: {e}", query.name);
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Error processing flow request",
                    )
                }
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

async fn flowsv3_v2(headers: axum::http::HeaderMap, Query(query): Query<FlowQueryV3>) -> Response {
    match rebundle_and_read_flow(&query.alias, &request_public_url(&headers)).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            let (status, message) = match e.as_str() {
                "Flow not found" => (axum::http::StatusCode::NOT_FOUND, "Flow not found"),
                "Script file not found" => {
                    (axum::http::StatusCode::NOT_FOUND, "Script file not found")
                }
                _ => {
                    error!("Could not serve flow {}: {e}", query.alias);
                    (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Error processing flow request",
                    )
                }
            };
            (status, Json(serde_json::json!({ "error": message }))).into_response()
        }
    }
}

async fn health() -> &'static str {
    "healthy"
}

#[derive(Deserialize)]
struct VerifyQuery {
    #[serde(rename = "sessionActionId")]
    session_action_id: Option<String>,
}

async fn verify(headers: axum::http::HeaderMap, Query(query): Query<VerifyQuery>) -> Response {
    info!("Verifying request with headers: {:?}", headers);
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    info!("Auth header: {:?}", auth_header);
    let auth_provider = headers
        .get("authorization-provider")
        .and_then(|v| v.to_str().ok());
    info!("Auth provider: {:?}", auth_provider);

    // Check if required headers are present
    if auth_header.is_none() || auth_provider.is_none() {
        info!("Missing required headers");
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing required headers" })),
        )
            .into_response();
    }

    let Some(session_action_id) = query.session_action_id else {
        info!("Missing sessionActionId");
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing sessionActionId" })),
        )
            .into_response();
    };

    verified_flow(&session_action_id)
}

/// `/v3/flows` mints session action ids that name the flow, so `/verify` can
/// tell the notary which flow a session action belongs to.
const FLOW_SESSION_ACTION_PREFIX: &str = "flow:";

fn session_action_id_for(alias: &str) -> String {
    format!("{FLOW_SESSION_ACTION_PREFIX}{alias}")
}

/// The flow of a session action, as the notary reads it from `/verify`
#[derive(Serialize)]
struct VerifyFlow {
    alias: String,
    mapper_hash: Option<String>,
    mapper_url: Option<String>,
    allowed_hosts: Option<Vec<String>>,
}

/// The notary refuses a proof unless `/verify` names the flow of its session
/// action. It then applies that flow's mapper and allowed hosts.
fn verified_flow(session_action_id: &str) -> Response {
    let flow = session_action_id
        .strip_prefix(FLOW_SESSION_ACTION_PREFIX)
        .and_then(|alias| ALIAS_TO_FLOW_MAP_AND_PLATFORM_INDEX.get()?.get(alias));
    let Some((flow, _)) = flow else {
        info!("Unknown session action: {session_action_id}");
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session action not found" })),
        )
            .into_response();
    };

    let public_url = FLOW_PUBLIC_URLS
        .read()
        .unwrap()
        .get(&flow.alias)
        .cloned()
        .unwrap_or_else(default_url);

    match flow_mapper(&flow.alias, &public_url) {
        Ok(mapper) => {
            let (mapper_hash, mapper_url) = mapper.map(|mapper| (mapper.hash, mapper.url)).unzip();
            Json(serde_json::json!({
                "success": true,
                "message": "API key validated",
                "flow": VerifyFlow {
                    alias: flow.alias.clone(),
                    mapper_hash,
                    mapper_url,
                    allowed_hosts: flow.allowed_hosts.clone(),
                },
            }))
            .into_response()
        }
        Err(e) => {
            error!("{e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    }
}

async fn sessions() -> Json<SessionResponse> {
    Json(SessionResponse {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        api_key_id: "secret-1234".to_string(),
    })
}

static CONFIG_PATH: OnceLock<String> = OnceLock::new();
static SHOULD_REBUNDLE: OnceLock<bool> = OnceLock::new();
/// `--public-url`
static PUBLIC_URL: OnceLock<Url> = OnceLock::new();
/// The base URL when no request names one: `--public-url`, or
/// `http://localhost:<port>`
static DEFAULT_URL: OnceLock<Url> = OnceLock::new();

const PUBLIC_URL_SCHEME_ERROR: &str = "the URL must start with http:// or https://";

/// Checks a `--public-url` value: an http or https URL with a host and
/// nothing after it
pub fn parse_public_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| match error {
        url::ParseError::RelativeUrlWithoutBase => PUBLIC_URL_SCHEME_ERROR.to_string(),
        error => error.to_string(),
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(PUBLIC_URL_SCHEME_ERROR.into());
    }

    let scheme_and_host_only = url.has_host()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if !scheme_and_host_only {
        return Err("the URL must be a scheme and a host, such as http://192.168.1.10:8080".into());
    }
    Ok(url)
}

/// The default URL for `serve`: `--public-url`, or `http://localhost:<port>`
fn default_url_for(public_url: Option<&Url>, port: u16) -> Result<Url, url::ParseError> {
    match public_url {
        Some(public_url) => Ok(public_url.clone()),
        None => Url::parse(&format!("http://localhost:{port}")),
    }
}

fn default_url() -> Url {
    DEFAULT_URL
        .get()
        .cloned()
        .expect("serve sets the default URL before it serves any request")
}

/// This server's base URL as the caller reached it: `--public-url`, else the
/// `Host` the caller sent, else the default URL
fn request_public_url(headers: &axum::http::HeaderMap) -> Url {
    if let Some(public_url) = PUBLIC_URL.get() {
        return public_url.clone();
    }
    headers
        .get(axum::http::header::HOST)
        .and_then(|host| host.to_str().ok())
        .and_then(|host| parse_public_url(&format!("http://{host}")).ok())
        .unwrap_or_else(default_url)
}

/// `path` on this server at `base`. An http or https base joins any
/// relative path.
fn server_url(base: &Url, path: &str) -> String {
    base.join(path)
        .expect("an http or https base URL joins any relative path")
        .into()
}

/// The base URL `/v3/flows` gave the SDK, per flow alias. The SDK and the
/// notary both sign the mapper URL, so `/verify` must give the notary the same
/// string, even when the notary reaches this server at another address.
static FLOW_PUBLIC_URLS: LazyLock<RwLock<HashMap<String, Url>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn flows_config() -> Result<Config, String> {
    let config_path = CONFIG_PATH.get().map_or("./opacity.toml", String::as_str);
    Config::from_file(config_path).map_err(|e| format!("Could not read {config_path}: {e}"))
}

static HASH_TO_MAPPER: LazyLock<RwLock<HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The bundled mapper with this alias, or None when the server serves no
/// mappers
fn bundled_mapper_path(alias: &str) -> Option<PathBuf> {
    let mappers_folder = MAPPERS_FOLDER.get()?;
    let mappers_config = MAPPERS_CONFIG.get()?;
    Some(
        PathBuf::from(mappers_folder)
            .join(&mappers_config.settings.output_directory)
            .join(format!("{alias}.bundle.luau")),
    )
}

/// Hashes the bundled mapper with this alias and remembers the hash, so the
/// mapper routes can serve it. None when there is no such mapper.
fn index_mapper(alias: &str) -> std::io::Result<Option<String>> {
    let Some(file_path) = bundled_mapper_path(alias) else {
        return Ok(None);
    };
    let file_content = match fs::read_to_string(file_path) {
        Ok(file_content) => file_content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };

    let sha256 = get_sha256(&file_content);
    HASH_TO_MAPPER
        .write()
        .unwrap()
        .insert(sha256.clone(), alias.to_owned());
    Ok(Some(sha256))
}

struct FlowMapper {
    hash: String,
    url: String,
}

/// The flow's mapper is the bundled mapper with the flow's alias, as the
/// backend pairs them. Its URL is on `public_url`.
fn flow_mapper(alias: &str, public_url: &Url) -> Result<Option<FlowMapper>, String> {
    let hash = index_mapper(alias)
        .map_err(|e| format!("Could not read the mapper of flow {alias}: {e}"))?;

    Ok(hash.map(|hash| FlowMapper {
        url: server_url(public_url, &format!("mapper/{hash}")),
        hash,
    }))
}

async fn resolve_mapper_alias(
    headers: axum::http::HeaderMap,
    Query(query): Query<ResolveMapperAliasQuery>,
) -> Response {
    match index_mapper(&query.alias) {
        Ok(Some(sha256)) => Json(MapperResponse {
            mapper_url: server_url(
                &request_public_url(&headers),
                &format!("resolve_mapper_file?hash={sha256}"),
            ),
            mapper_hash: sha256,
        })
        .into_response(),
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Mapper not found").into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn resolve_mapper_file(Query(query): Query<ResolveMapperFileQuery>) -> Response {
    mapper_file_response(&query.hash)
}

/// Serves the mapper URL that `/v3/flows` and `/verify` give for a flow
async fn mapper_by_hash(Path(hash): Path<String>) -> Response {
    mapper_file_response(&hash)
}

fn mapper_file_response(hash: &str) -> Response {
    let alias = HASH_TO_MAPPER.read().unwrap().get(hash).cloned();
    let Some(file_path) = alias.as_deref().and_then(bundled_mapper_path) else {
        return (axum::http::StatusCode::NOT_FOUND, "Mapper not found").into_response();
    };

    match fs::read_to_string(file_path) {
        Ok(file_content) if get_sha256(&file_content) == hash => {
            (axum::http::StatusCode::OK, file_content).into_response()
        }
        Ok(_) => (
            axum::http::StatusCode::NOT_FOUND,
            "Mapper file content does not match hash, file might've been modified, fetch the flow again",
        )
            .into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// sha256 function used when hashing mapper files in the opacity sdk
pub fn get_sha256(payload: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    let normalized = payload.replace("\r\n", "\n");
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn initialize_mappers() -> Result<()> {
    let mappers_folder = "./mapping";
    match fs::metadata(mappers_folder) {
        Ok(metadata) => anyhow::ensure!(metadata.is_dir(), "mapping must be a directory"),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            info!("No mapping directory found, not serving mappers");
            return Ok(());
        }
        Err(error) => return Err(error).context("Failed to access mapping directory"),
    }

    let config = Config::from_file("./mapping/opacity.toml").context(
        "Failed to load mapping/opacity.toml. Run `cargo xtask aggregate` in mapping/ to generate it",
    )?;

    MAPPERS_CONFIG
        .set(config)
        .map_err(|_| anyhow::anyhow!("Mapper config already initialized"))?;
    MAPPERS_FOLDER
        .set(mappers_folder.to_owned())
        .map_err(|_| anyhow::anyhow!("Mapper folder already initialized"))?;
    info!("Serving mappers from mapping/");

    Ok(())
}

fn routes() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v2/flows", get(flows))
        .route("/v3/flows", get(flowsv3_v2))
        .route("/sessions", post(sessions))
        // SDKs up to v6.27.2 resolve a mapper by alias with these two routes
        .route("/resolve_mapper_alias", get(resolve_mapper_alias)) // this returns { mapper_hash: String, mapper_url: String }
        .route("/resolve_mapper_file", get(resolve_mapper_file)) // this returns the file content
        .route("/mapper/{hash}", get(mapper_by_hash)) // the mapper URL of /v3/flows and /verify
        .route("/verify", get(verify))
}

pub async fn serve(
    config_path: &str,
    should_rebundle: bool,
    port: u16,
    public_url: Option<Url>,
) -> Result<(), Box<dyn std::error::Error>> {
    // initialize everything
    get_alias_to_flow_map_and_platform_index(&Config::from_file(config_path).unwrap());
    CONFIG_PATH.get_or_init(|| config_path.to_owned());
    SHOULD_REBUNDLE.get_or_init(|| should_rebundle);
    let default_url = default_url_for(public_url.as_ref(), port)?;
    DEFAULT_URL.get_or_init(|| default_url);
    if let Some(public_url) = public_url {
        PUBLIC_URL.get_or_init(|| public_url);
    }

    initialize_mappers()?;

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

    let app = routes().layer(middleware);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    const MAPPER: &str = "return function(params)\n  return params.payload\nend\n";
    /// `shasum -a 256` of `MAPPER`, the hash the notary checks the download against
    const MAPPER_HASH: &str = "729bac902d2df2f35e9af4a7132cae240eedafdb8303b0869aa81f4360675f9b";
    /// The host the SDK uses, for example a device on the local network
    const SDK_HOST: &str = "192.168.1.10:8080";
    /// The host the notary uses, on the machine that runs the server
    const NOTARY_HOST: &str = "localhost:8080";

    /// Sets the server up as `serve` does without `--public-url`, for a project
    /// with a flow that has a mapper and allowed hosts, a flow with neither,
    /// and a flow with a mapper that the SDK never fetches
    fn serve_fixture() {
        static FIXTURE: OnceLock<()> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            let root = std::env::temp_dir().join(format!("opacity-cli-{}", Uuid::new_v4()));
            let bundled = root.join("bundled");
            let mapping = root.join("mapping");
            fs::create_dir_all(&bundled).unwrap();
            fs::create_dir_all(mapping.join("bundled")).unwrap();
            for alias in ["test:mapped", "test:unmapped", "test:unfetched"] {
                fs::write(bundled.join(format!("{alias}.bundle.luau")), "return 1\n").unwrap();
            }
            for alias in ["test:mapped", "test:unfetched"] {
                fs::write(mapping.join(format!("bundled/{alias}.bundle.luau")), MAPPER).unwrap();
            }

            let config_path = root.join("opacity.toml");
            let config_toml = format!(
                r#"
[settings]
output_directory = "{}"

[[platforms]]
name = "Test"
description = "Test platform"
flows = [
    {{ name = "Mapped", alias = "test:mapped", description = "", path = "src/mapped.luau", allowed_hosts = ["api.example.com"] }},
    {{ name = "Unmapped", alias = "test:unmapped", description = "", path = "src/unmapped.luau" }},
    {{ name = "Unfetched", alias = "test:unfetched", description = "", path = "src/unfetched.luau" }},
]
"#,
                bundled.display()
            );
            fs::write(&config_path, config_toml).unwrap();
            let config_path = config_path.to_string_lossy().into_owned();

            get_alias_to_flow_map_and_platform_index(&Config::from_file(&config_path).unwrap());
            CONFIG_PATH.get_or_init(|| config_path);
            SHOULD_REBUNDLE.get_or_init(|| false);
            DEFAULT_URL.get_or_init(|| default_url_for(None, 8080).unwrap());
            MAPPERS_FOLDER.get_or_init(|| mapping.to_string_lossy().into_owned());
            MAPPERS_CONFIG.get_or_init(|| Config {
                settings: Settings {
                    output_directory: "bundled".into(),
                    definition_files: None,
                },
                platforms: vec![],
            });
        });
    }

    /// Sends a GET to `host` with the headers the notary sends to `/verify`
    async fn get(host: &str, uri: &str) -> (StatusCode, Vec<u8>) {
        serve_fixture();
        let request = Request::get(uri)
            .header("host", host)
            .header("authorization", "Bearer key")
            .header("authorization-provider", "opacity")
            .body(Body::empty())
            .unwrap();
        let response = routes().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, body.to_vec())
    }

    async fn get_json(host: &str, uri: &str) -> (StatusCode, Value) {
        let (status, body) = get(host, uri).await;
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn verify_gives_the_notary_the_flow_rules_the_sdk_got() {
        let (status, flow) = get_json(SDK_HOST, "/v3/flows?alias=test:mapped").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(flow["sessionActionId"], "flow:test:mapped");
        assert_eq!(flow["mapperHash"], MAPPER_HASH);
        assert_eq!(
            flow["mapperUrl"],
            format!("http://{SDK_HOST}/mapper/{MAPPER_HASH}")
        );
        assert_eq!(flow["allowedHosts"], json!(["api.example.com"]));

        let (status, verified) =
            get_json(NOTARY_HOST, "/verify?sessionActionId=flow:test:mapped").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            verified["flow"],
            json!({
                "alias": "test:mapped",
                "mapper_hash": flow["mapperHash"],
                "mapper_url": flow["mapperUrl"],
                "allowed_hosts": flow["allowedHosts"],
            })
        );
    }

    #[tokio::test]
    async fn the_mapper_url_serves_the_bundled_mapper() {
        let (_, verified) = get_json(NOTARY_HOST, "/verify?sessionActionId=flow:test:mapped").await;
        let mapper_url = verified["flow"]["mapper_url"].as_str().unwrap();
        let mapper_path = &mapper_url[mapper_url.find("/mapper/").unwrap()..];

        let (status, body) = get(NOTARY_HOST, mapper_path).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, MAPPER.as_bytes());
    }

    #[tokio::test]
    async fn a_flow_the_sdk_did_not_fetch_uses_the_default_url() {
        let (status, verified) =
            get_json(NOTARY_HOST, "/verify?sessionActionId=flow:test:unfetched").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            verified["flow"]["mapper_url"],
            format!("http://localhost:8080/mapper/{MAPPER_HASH}")
        );
    }

    #[tokio::test]
    async fn a_flow_without_mapper_or_hosts_names_neither() {
        let (status, flow) = get_json(SDK_HOST, "/v3/flows?alias=test:unmapped").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(flow["sessionActionId"], "flow:test:unmapped");
        assert_eq!(flow["mapperHash"], Value::Null);
        assert_eq!(flow["mapperUrl"], Value::Null);
        assert_eq!(flow["allowedHosts"], Value::Null);

        let (status, verified) =
            get_json(NOTARY_HOST, "/verify?sessionActionId=flow:test:unmapped").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            verified["flow"],
            json!({
                "alias": "test:unmapped",
                "mapper_hash": null,
                "mapper_url": null,
                "allowed_hosts": null,
            })
        );
    }

    #[tokio::test]
    async fn verify_refuses_a_session_action_this_server_did_not_mint() {
        for session_action_id in ["dummy-action-id", "flow:test:missing"] {
            let (status, _) = get(
                NOTARY_HOST,
                &format!("/verify?sessionActionId={session_action_id}"),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{session_action_id}");
        }
    }

    #[test]
    fn the_default_url_is_the_public_url_flag_or_localhost() {
        let flag = parse_public_url("http://cli.example.com").unwrap();
        assert_eq!(default_url_for(Some(&flag), 9090), Ok(flag));
        assert_eq!(
            default_url_for(None, 9090).map(String::from),
            Ok("http://localhost:9090/".to_string())
        );
    }

    #[test]
    fn a_public_url_is_a_scheme_and_a_host() {
        assert_eq!(
            parse_public_url("http://192.168.1.10:8080").map(String::from),
            Ok("http://192.168.1.10:8080/".to_string())
        );
        assert_eq!(
            parse_public_url("https://cli.example.com/").map(String::from),
            Ok("https://cli.example.com/".to_string())
        );
        assert_eq!(
            parse_public_url("192.168.1.10:8080"),
            Err(PUBLIC_URL_SCHEME_ERROR.to_string())
        );
        for value in [
            "localhost:8080",
            "ftp://host",
            "http://",
            "http://host/mapper",
            "http://host/?debug=1",
            "http://user@host",
        ] {
            assert!(parse_public_url(value).is_err(), "{value}");
        }
    }
}
