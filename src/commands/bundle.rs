use crate::config::Flow;
use crate::config::{self, SimplePlatform};

use anyhow::Result;
use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{InjectGlobalValue, RemoveComments, Rule};
use darklua_core::{
    process, BundleConfiguration, Configuration, GeneratorParameters, Options, Resources,
    WorkerTree,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::info;

const SOURCE_MAP_PREFIX: &str = "--@opacity-sourcemap ";

#[derive(Serialize)]
struct SourceMapFile {
    path: String,
    start: usize,
    end: usize,
}

fn relative_path(path: &Path, cwd: &Path) -> String {
    let relative = if path.is_absolute() {
        path.strip_prefix(cwd)
            .ok()
            .or_else(|| path.file_name().map(Path::new))
    } else {
        Some(path)
    };
    relative
        .map(|path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches("./")
                .to_string()
        })
        .unwrap_or_default()
}

fn source_map_line(tree: &WorkerTree, bundle_line_count: usize) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let mut files: Vec<SourceMapFile> = tree
        .iter_bundle_source_ranges()
        .flat_map(|(_, ranges)| ranges)
        .map(|range| SourceMapFile {
            path: relative_path(&range.path, &cwd),
            start: range.start_line,
            end: range.end_line.min(bundle_line_count),
        })
        .collect();
    files.sort_by_key(|file| file.start);
    Ok(format!(
        "{SOURCE_MAP_PREFIX}{}\n",
        serde_json::to_string(&files)?
    ))
}

fn get_global_inject_rules(platform: &SimplePlatform, flow: &Flow) -> Vec<Box<dyn Rule>> {
    let mut rules: Vec<Box<dyn Rule>> = vec![
        Box::new(RemoveComments::default()),
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

pub fn process_bundle(resources: &Resources, bundle_options: BundleOptions) -> Result<()> {
    let process_start = Instant::now();
    let result = process(resources, bundle_options.opts)
        .map_err(|e| anyhow::anyhow!("Processing failed: {:?}", e))?;
    if let Some(err) = result.collect_errors().first() {
        anyhow::bail!("Failed to process: {:?}", err);
    }

    let mut bundle = std::fs::read_to_string(&bundle_options.output)?;
    if !bundle.ends_with('\n') {
        bundle.push('\n');
    }
    let source_map = source_map_line(&result, bundle.lines().count())?;
    bundle.push_str(&source_map);
    std::fs::write(&bundle_options.output, bundle)?;

    println!("Successfully processed in {:?}", process_start.elapsed());
    Ok(())
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
            .with_generator_override(GeneratorParameters::RetainLines)
            .with_configuration(config),
        output: output.clone(),
    })
}

pub fn bundle(config_path: &str, is_rebundle: bool) -> Result<()> {
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    std::fs::create_dir_all(&config.settings.output_directory)?;

    let mut file_paths: Vec<PathBuf> = Vec::new();

    for platform in &config.platforms {
        println!("Processing platform: {}", platform.name);
        let simple_platform = SimplePlatform::from(platform);

        for flow in &platform.flows {
            println!("Bundling {} ({})", flow.name, flow.alias);

            let bundle_options = create_options(&config, &simple_platform, flow)?;

            file_paths.push(bundle_options.output.clone());

            process_bundle(&resources, bundle_options)?;
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

    if is_rebundle {
        info!("Rebundled all flows successfully");
    } else {
        info!("Bundled all flows successfully");
    }

    Ok(())
}
