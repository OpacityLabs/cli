use crate::config;
use crate::config::{Flow, Platform};

use anyhow::Result;
use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{InjectGlobalValue, Rule};
use darklua_core::{
    process, BundleConfiguration, Configuration, GeneratorParameters, Options, Resources,
};
use std::path::PathBuf;
use std::time::Instant;
use tracing::info;

pub mod param_extractor;

fn get_global_inject_rules(platform: &Platform, flow: &Flow) -> Vec<Box<dyn Rule>> {
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

fn process_bundle(resources: &Resources, options: Options) -> Result<()> {
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

fn serialize_flows_with_param_variants_to_toml(
    flows_with_params: &Vec<(String, Vec<param_extractor::ParamVariant>)>,
) -> String {
    use toml_edit::*;
    let mut doc = DocumentMut::new();

    let flows_with_params = flows_with_params
        .iter()
        .map(|(alias, param_variants)| {
            let mut table = Table::new();
            table.insert("alias", alias.clone().into());

            let params = param_variants
                .iter()
                .map(|param_variant| {
                    param_variant
                        .iter()
                        .map(|param| param.to_toml_table().into_inline_table())
                        .collect::<Array>()
                })
                .collect::<Array>();

            table.insert("params", params.into());
            table
        })
        .collect::<ArrayOfTables>();

    doc.insert("flows_with_params", flows_with_params.into());
    doc.to_string()
}

pub fn bundle(config_path: &str, is_rebundle: bool) -> Result<()> {
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    let mut flows_with_params: Vec<(String, Vec<param_extractor::ParamVariant>)> = Vec::new();

    std::fs::create_dir_all(&config.settings.output_directory)?;

    let mut file_paths: Vec<PathBuf> = Vec::new();

    for platform in &config.platforms {
        println!("Processing platform: {}", platform.name);

        for flow in &platform.flows {
            println!("Bundling {} ({})", flow.name, flow.alias);
            let input = PathBuf::from(&flow.path);

            let output = PathBuf::from(&config.settings.output_directory)
                .join(format!("{}.bundle.luau", flow.alias));

            file_paths.push(output.clone());

            let mut config = Configuration::empty();
            config = config.with_bundle_configuration(
                BundleConfiguration::new(BundleRequireMode::Path(Default::default()))
                    .with_modules_identifier("__BUNDLE_MODULES"),
            );

            let rules = get_global_inject_rules(platform, flow);

            for rule in rules {
                config = config.with_rule(rule);
            }

            let options = Options::new(&input)
                .with_output(&output)
                .with_generator_override(GeneratorParameters::Dense { column_span: 80 })
                .with_configuration(config);

            process_bundle(&resources, options)?;

            let parent_path_as_cwd = std::fs::canonicalize(config_path)
                .ok()
                .and_then(|abs_path| {
                    abs_path
                        .parent()
                        .map(|p| p.to_str().map(|s| s.to_owned()))
                        .flatten()
                })
                .map(|s| s.to_string());

            flows_with_params.push((
                flow.alias.clone(),
                param_extractor::extract_params(
                    &std::fs::read_to_string(&flow.path)?,
                    &flow.path,
                    parent_path_as_cwd,
                )?,
            ));
        }
    }

    let hashes = compute_hashes(&mut file_paths)?;

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

    std::fs::write(
        config_path_dir_buf.join("flows_with_params.toml"),
        serialize_flows_with_param_variants_to_toml(
            &flows_with_params
                .iter()
                .filter_map(|(alias, param_variants)| {
                    if param_variants.is_empty() {
                        None
                    } else {
                        Some((alias.clone(), param_variants.clone()))
                    }
                })
                .collect::<Vec<(String, Vec<param_extractor::ParamVariant>)>>(),
        ),
    )?;

    if is_rebundle {
        info!("Rebundled all flows successfully");
    } else {
        info!("Bundled all flows successfully");
    }

    Ok(())
}
