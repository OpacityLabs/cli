use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub settings: Settings,
    pub platforms: Vec<Platform>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub output_directory: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Platform {
    pub name: String,
    pub description: String,
    pub flows: Vec<Flow>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Flow {
    pub name: String,
    pub alias: String,
    pub description: String,
    #[serde(rename = "minSdkVersion")]
    pub min_sdk_version: String,
    pub retrieves: Vec<String>,
    pub path: String,
}

impl Config {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
