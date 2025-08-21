use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub settings: Settings,
    pub platforms: Vec<Platform>,
}

impl Config {
    pub fn serialize_to_toml_document(&self) -> toml_edit::DocumentMut {
        use toml_edit::*;
        let mut doc = DocumentMut::new();
        doc.insert("settings", self.settings.serialize_to_toml().into());

        doc.insert(
            "platforms",
            Item::ArrayOfTables(ArrayOfTables::from_iter(
                self.platforms
                    .iter()
                    .map(|platform| platform.serialize_to_toml()),
            )),
        );
        doc
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub output_directory: String,
    pub definition_files: Option<Vec<String>>,
}

impl Settings {
    pub fn serialize_to_toml(&self) -> toml_edit::Table {
        use toml_edit::*;
        let mut table = Table::new();
        table.insert("outputDirectory", self.output_directory.clone().into());
        if let Some(definition_files) = self.definition_files.clone() {
            let mut arr = Array::new();
            for definition_file in definition_files {
                arr.push::<String>(definition_file.clone().into());
            }
        }
        table
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Platform {
    pub name: String,
    pub description: String,
    pub flows: Vec<Flow>,
}

impl Platform {
    pub fn serialize_to_toml(&self) -> toml_edit::Table {
        use toml_edit::*;
        let mut table = Table::new();
        table.insert("name", self.name.clone().into());
        table.insert("description", self.description.clone().into());
        let mut array = Array::new();
        for flow in self.flows.iter() {
            array.push::<Value>(toml_edit::Value::InlineTable(
                flow.serialize_to_toml().into_inline_table(),
            ));
        }
        table.insert("flows", array.into());
        table
    }
}

type ParamType = String;
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Param {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    // we keep it as a String for now
    // maybe we move to a String | Table later :)
    pub ty: ParamType,
    pub required: bool,
}

impl Param {
    pub fn serialize_to_toml(&self) -> toml_edit::Table {
        use toml_edit::*;
        let mut table = Table::new();
        table.insert("name", self.name.clone().into());
        table.insert("description", self.description.clone().into());
        table.insert("type", self.ty.clone().into());
        table.insert("required", self.required.clone().into());
        table
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
    // we'll actually write these params in the opacity.toml file based on the flow's params
    pub params: Option<Vec<Param>>,
}

impl Flow {
    pub fn serialize_to_toml(&self) -> toml_edit::Table {
        use toml_edit::*;
        let mut table = Table::new();

        table.insert("name", self.name.clone().into());
        table.insert("alias", self.alias.clone().into());
        table.insert("description", self.description.clone().into());
        if let Some(min_sdk_version) = self.min_sdk_version.clone() {
            table.insert("minSdkVersion", min_sdk_version.into());
        }
        if let Some(retrieves) = self.retrieves.clone() {
            let mut arr = Array::new();
            for retrieve in retrieves {
                arr.push::<String>(retrieve.clone().into());
            }
            table.insert("retrieves", arr.into());
        }
        table.insert("path", self.path.clone().into());
        if let Some(params) = self.params.clone() {
            let mut arr = Array::new();
            for param in params {
                arr.push::<Value>(toml_edit::Value::InlineTable(
                    param.serialize_to_toml().into_inline_table(),
                ));
            }
            table.insert("params", arr.into());
        }
        table
    }
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
