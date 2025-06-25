use crate::config;
use crate::config::{Flow, Platform};

use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{
    InjectGlobalValue, RemoveCompoundAssignment, RemoveContinue, RemoveIfExpression, RemoveTypes,
    Rule,
};
use darklua_core::{
    process, BundleConfiguration, Configuration, GeneratorParameters, Options, Resources,
};
use std::path::PathBuf;
use std::time::Instant;
use anyhow::Result;
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

pub fn bundle(config_path: &str, is_rebundle: bool) -> Result<()> {
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    std::fs::create_dir_all(&config.settings.output_directory)?;

    for platform in &config.platforms {
        println!("Processing platform: {}", platform.name);

        for flow in &platform.flows {
            println!("Bundling {} ({})", flow.name, flow.alias);
            let input = PathBuf::from(&flow.path);

            let output = PathBuf::from(&config.settings.output_directory)
                .join(format!("{}.bundle.lua", flow.alias));

            let mut config = Configuration::empty();
            config = config.with_bundle_configuration(
                BundleConfiguration::new(BundleRequireMode::Path(Default::default()))
                    .with_modules_identifier("__BUNDLE_MODULES"),
            );

            let rules: Vec<Box<dyn Rule>> = vec![
                Box::new(RemoveContinue::default()),
                Box::new(RemoveCompoundAssignment::default()),
                Box::new(RemoveTypes::default()),
                Box::new(RemoveIfExpression::default()),
            ];
            let rules = rules
                .into_iter()
                .chain(get_global_inject_rules(platform, flow))
                .collect::<Vec<Box<dyn Rule>>>();

            for rule in rules {
                config = config.with_rule(rule);
            }

            let options = Options::new(&input)
                .with_output(&output)
                .with_generator_override(GeneratorParameters::RetainLines)
                .with_configuration(config);

            process_bundle(&resources, options)?;
        }
    }

    if is_rebundle {
        info!("Rebundled all flows successfully");
    } else {
        info!("Bundled all flows successfully");
    }

    Ok(())
}