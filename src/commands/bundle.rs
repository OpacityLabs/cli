use crate::config;
use crate::config::{Flow, Platform};

mod custom_process_bundle;

use anyhow::Result;
use custom_process_bundle::custom_process_bundle;
use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{InjectGlobalValue, Rule};
use darklua_core::{
    process, BundleConfiguration, Configuration, GeneratorParameters, Options, Resources,
};
use std::path::PathBuf;
use std::time::Instant;
use tracing::info;

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

pub fn bundle(config_path: &str, is_rebundle: bool) -> Result<()> {
    let toml_config = &mut config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    std::fs::create_dir_all(&toml_config.settings.output_directory)?;

    let mut file_paths: Vec<PathBuf> = Vec::new();

    for platform in &mut toml_config.platforms {
        println!("Processing platform: {}", platform.name);

        let cloned_platform = platform.clone();

        for flow in &mut platform.flows {
            println!("Bundling {} ({})", flow.name, flow.alias);
            let input = PathBuf::from(&flow.path);

            let output = PathBuf::from(&toml_config.settings.output_directory)
                .join(format!("{}.bundle.luau", flow.alias));

            file_paths.push(output.clone());

            let mut config = Configuration::empty();
            config = config.with_bundle_configuration(
                BundleConfiguration::new(BundleRequireMode::Path(Default::default()))
                    .with_modules_identifier("__BUNDLE_MODULES"),
            );

            let rules = get_global_inject_rules(&cloned_platform, flow);

            for rule in rules {
                config = config.with_rule(rule);
            }

            let options = Options::new(&input)
                .with_output(&output)
                .with_generator_override(GeneratorParameters::Dense { column_span: 80 })
                .with_configuration(config);

            process_bundle(&resources, options)?;
            custom_process_bundle(flow)?;
        }
    }

    // Don't forget to write the config file (with the new added params)
    std::fs::write(config_path, toml_config.serialize_to_toml_document().to_string())?;

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

    if is_rebundle {
        info!("Rebundled all flows successfully");
    } else {
        info!("Bundled all flows successfully");
    }

    Ok(())
}

#[test]
fn make_sure_toml_inlines_tables() -> Result<()> {
    let config_as_str = r#"[settings]
output_directory = "bundled"
definition_files = ["luau-global-types/_types/global.d.luau"]

[[platforms]]
name = "Bybit"
slug = "bybit"
description = "Cryptocurrency derivatives exchange"
logoUrl = "https://assets.opacity.network/bybit-logo.png"
logoColor = "\\#F7931A"
status = "live"
workingStatus = "active"
flows = [
    { name = "Borrow History", alias = "bybit:account:borrow_history", description = "Get borrow history", minSdkVersion = "1", retrieves = [
        "borrow_history_data",
    ], path = "src/bybit_com/flows/account/borrow_history.luau" },
    { name = "Coin Greeks", alias = "bybit:account:coin_greeks", description = "Get coin greeks data", minSdkVersion = "1", retrieves = [
        "coin_greeks_data",
    ], path = "src/bybit_com/flows/account/coin-greeks.luau" },
]

[[platforms]]
name = "Test"
slug = "test"
description = "Test"
logoUrl = "https://assets.opacity.network/test-logo.png"
logoColor = "\\#FFFFFF"
status = "hidden"
workingStatus = "active"
flows = [
    { name = "Open Browser", alias = "test:open_browser_must_succeed", description = "Open browser", path = "src/test/open_browser_must_succeed.luau", minSdkVersion = "1", retrieves = [
        "browser_data",
    ] },
    { name = "Mock error", alias = "test:error", description = "Mock an error being thrown.", path = "src/test/mock_error.luau", minSdkVersion = "1", retrieves = [
        "random_data",
    ] },
]"#;

    let mut config: config::Config = toml::from_str(config_as_str)?;
    let serialized_config = config.serialize_to_toml_document().to_string();

    assert!(serialized_config.find("[[platforms.flows]]").is_none());
    assert!(serialized_config.find("[[platforms.flows.params]]").is_none());

    config.platforms[0].flows[0].params = Some(vec![
        config::Param {
            name: "test".to_string(),
            description: "test".to_string(),
            ty: "string".to_string(),
            required: true,
        }
    ]);

    let no_toml_edit_serialized_config = toml::to_string(&config)?;

    println!("{}", no_toml_edit_serialized_config);
    assert!(no_toml_edit_serialized_config.find("[[platforms.flows]]").is_some());
    assert!(no_toml_edit_serialized_config.find("[[platforms.flows.params]]").is_some());

    Ok(())
}
