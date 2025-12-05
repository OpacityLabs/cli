use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use darklua_core::Resources;

use crate::{
    commands::version::{
        dependency_graph::{DepedencyGraph, Work},
        version_visitor::VersionFile,
    },
    config,
};

pub mod dependency_graph;
mod dependency_visitor;
mod has_call_to_function_visitor;
pub mod sdk_version;
mod utils;
pub mod version_visitor;

pub fn compute_version_for_flows<'a>(
    resources: &Resources,
    flow_paths: Vec<PathBuf>,
    version_file: VersionFile,
) -> Result<Work<'_>> {
    let graph = DepedencyGraph::new();
    let mut work = Work::new(graph, resources, flow_paths, version_file);
    work.compute_dependency_graph()
        .map_err(|e| anyhow::anyhow!("Failed to compute dependency graph: {:?}", e))?;

    Ok(work)
}

pub fn compute_versions(config_path: &str) -> Result<()> {
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    let mut file_paths: Vec<PathBuf> = Vec::new();

    let mut path_to_alias = HashMap::new();

    for platform in &config.platforms {
        for flow in &platform.flows {
            let input = PathBuf::from(&flow.path);
            path_to_alias.insert(input.clone(), flow.alias.clone());
            file_paths.push(input.clone());
        }
    }

    let mut config_path_dir_buf = PathBuf::from(config_path);
    config_path_dir_buf.pop();
    let version_file: VersionFile = serde_json::from_str(
        &std::fs::read_to_string(config_path_dir_buf.join("version_file.json")).map_err(|e| {
            anyhow::anyhow!("Failed to read version file (version_file.json): {:?}", e)
        })?,
    )?;

    let work = compute_version_for_flows(&resources, file_paths, version_file)?;

    let versions = work.get_versions();

    // finally, modify the versions HashMap to have Alias->Version instead of Path->Version
    let mut alias_versions = HashMap::new();
    for (path, version) in &versions {
        let alias = path_to_alias.get(path).unwrap();
        alias_versions.insert(alias.clone(), version.clone());
    }

    let mut config_path_dir_buf = PathBuf::from(config_path);
    config_path_dir_buf.pop();
    std::fs::write(
        config_path_dir_buf.join("versions.lock"),
        serde_json::to_string(&alias_versions.clone())?,
    )?;

    Ok(())
}
