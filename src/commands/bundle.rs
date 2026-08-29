use crate::config::Flow;
use crate::config::{self, SimplePlatform};

use anyhow::Result;
use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{InjectGlobalValue, Rule};
use darklua_core::{
    process, BundleConfiguration, Configuration, GeneratorParameters, Options, Resources,
};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{info, warn};

fn get_global_inject_rules(platform: &SimplePlatform, flow: &Flow) -> Vec<Box<dyn Rule>> {
    let mut rules: Vec<Box<dyn Rule>> = vec![
        Box::new(InjectGlobalValue::string("FLOW_NAME", flow.name.clone())),
        Box::new(InjectGlobalValue::string("FLOW_ALIAS", flow.alias.clone())),
        Box::new(InjectGlobalValue::string(
            "PLATFORM_NAME",
            platform.name.clone(),
        )),
        Box::new(InjectGlobalValue::string(
            "PLATFORM_DESCRIPTION",
            platform.description.clone(),
        )),
    ];

    if let Some(min_sdk_version) = &flow.min_sdk_version {
        rules.push(Box::new(InjectGlobalValue::string(
            "MIN_SDK_VERSION",
            min_sdk_version.clone(),
        )));
    }

    if let Some(retrieves) = &flow.retrieves {
        rules.push(Box::new(InjectGlobalValue::string(
            "RETRIEVES",
            retrieves.join(", "),
        )));
    }

    rules
}

pub fn process_bundle(resources: &Resources, options: Options) -> Result<()> {
    let process_start = Instant::now();
    let result =
        process(resources, options).map_err(|e| anyhow::anyhow!("Processing failed: {:?}", e))?;

    match result.result() {
        Ok(_) => {
            println!("Successfully processed in {:?}", process_start.elapsed());
            Ok(())
        }
        Err(err) => {
            anyhow::bail!("Failed to process: {:?}", err);
        }
    }
}

fn compute_hashes(file_paths: &mut Vec<PathBuf>) -> Result<Vec<(String, String)>> {
    file_paths.sort();
    use sha2::Digest;

    let mut hashes: Vec<(String, String)> = Vec::new();
    for file_path in file_paths {
        let file_content = std::fs::read(file_path.clone())?;
        let hash = format!("{:x}", sha2::Sha256::digest(&file_content));
        hashes.push((file_path.to_string_lossy().to_string(), hash));
    }

    Ok(hashes)
}

pub struct BundleOptions {
    pub opts: Options,
    pub output: PathBuf,
}

pub fn create_options(
    config: &config::Config,
    platform: &SimplePlatform,
    flow: &Flow,
) -> Result<BundleOptions> {
    std::fs::create_dir_all(&config.settings.output_directory)?;
    let input = PathBuf::from(&flow.path);

    let output = PathBuf::from(&config.settings.output_directory)
        .join(format!("{}.bundle.luau", flow.alias));

    let mut config = Configuration::empty();
    config = config.with_bundle_configuration(
        BundleConfiguration::new(BundleRequireMode::Path(Default::default()))
            .with_modules_identifier("__BUNDLE_MODULES"),
    );

    let rules = get_global_inject_rules(platform, flow);

    for rule in rules {
        config = config.with_rule(rule);
    }

    Ok(BundleOptions {
        opts: Options::new(&input)
            .with_output(&output)
            .with_generator_override(GeneratorParameters::Dense { column_span: 80 })
            .with_configuration(config),
        output: output.clone(),
    })
}

fn flow_matches(flow: &Flow, flow_name: &str) -> bool {
    flow.alias == flow_name
}

fn write_hashes_lock(config_path: &str, file_paths: &mut Vec<PathBuf>) -> Result<()> {
    let hashes = compute_hashes(file_paths)?;

    let mut config_path_dir_buf = PathBuf::from(config_path);
    config_path_dir_buf.pop();
    std::fs::write(
        config_path_dir_buf.join("hashes.lock"),
        hashes
            .iter()
            .map(|(path, hash)| format!("{}:{}", path, hash))
            .collect::<Vec<String>>()
            .join("\n"),
    )?;

    Ok(())
}

/// Bundles every flow in the config, or only the one matching `flow_name` (by alias).
///
/// `hashes.lock` is only written on a full bundle: a single-flow run would otherwise leave
/// a manifest that no longer describes the rest of the output directory.
pub fn bundle(config_path: &str, is_rebundle: bool, flow_name: Option<&str>) -> Result<()> {
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    std::fs::create_dir_all(&config.settings.output_directory)?;

    let mut file_paths: Vec<PathBuf> = Vec::new();

    for platform in &config.platforms {
        let flows: Vec<&Flow> = platform
            .flows
            .iter()
            .filter(|flow| flow_name.is_none_or(|name| flow_matches(flow, name)))
            .collect();

        if flows.is_empty() {
            continue;
        }

        println!("Processing platform: {}", platform.name);
        let simple_platform = SimplePlatform::from(platform);

        for flow in flows {
            println!("Bundling {} ({})", flow.name, flow.alias);

            let bundle_options = create_options(&config, &simple_platform, flow)?;

            file_paths.push(bundle_options.output.clone());

            process_bundle(&resources, bundle_options.opts)?;
        }
    }

    match flow_name {
        Some(name) => {
            if file_paths.is_empty() {
                anyhow::bail!(
                    "No flow named '{}' in {}. Available flows (alias): {}",
                    name,
                    config_path,
                    config
                        .platforms
                        .iter()
                        .flat_map(|platform| platform.flows.iter())
                        .map(|flow| flow.alias.clone())
                        .collect::<Vec<String>>()
                        .join(", ")
                );
            }

            info!("Bundled flow '{}' successfully", name);
            warn!("The `hashes.lock` file won't be updated since only one flow was bundled!");
        }
        None => {
            write_hashes_lock(config_path, &mut file_paths)?;

            if is_rebundle {
                info!("Rebundled all flows successfully");
            } else {
                info!("Bundled all flows successfully");
            }
        }
    }

    Ok(())
}
