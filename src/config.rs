use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub settings: Settings,
    pub platforms: Vec<Platform>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub output_directory: String,
    pub definition_files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Platform {
    pub name: String,
    pub description: String,
    pub flows: Vec<Flow>,
}

#[derive(Debug, Clone)]
pub struct SimplePlatform {
    pub name: String,
    pub description: String,
}

impl From<Platform> for SimplePlatform {
    fn from(platform: Platform) -> Self {
        Self {
            name: platform.name,
            description: platform.description,
        }
    }
}

impl From<&Platform> for SimplePlatform {
    fn from(platform: &Platform) -> Self {
        Self {
            name: platform.name.clone(),
            description: platform.description.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Flow {
    pub name: String,
    pub alias: String,
    pub description: String,
    #[serde(rename = "minSdkVersion")]
    pub min_sdk_version: Option<String>,
    pub retrieves: Option<Vec<String>>,
    pub path: String,
}

impl Config {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn get_flows_paths(&self) -> Vec<String> {
        let mut files = Vec::new();
        let current_dir_path = std::env::current_dir().unwrap();
        let current_dir_path_str = std::path::Path::new(&current_dir_path);

        for platform in self.platforms.iter() {
            for flow in platform.flows.iter() {
                let flow_path = std::path::Path::new(&flow.path);
                files.push(
                    std::path::Path::join(current_dir_path_str, flow_path)
                        .to_str()
                        .unwrap()
                        .to_string(),
                );
            }
        }
        files
    }

    #[allow(dead_code)]
    pub fn get_flow(&self, flow_name: &str) -> Option<Flow> {
        for platform in self.platforms.iter() {
            for flow in platform.flows.iter() {
                if flow.name == flow_name {
                    return Some(flow.clone());
                }
            }
        }
        None
    }
}
