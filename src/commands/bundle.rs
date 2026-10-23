use crate::config::Flow;
use crate::config::{self, SimplePlatform};

use anyhow::Result;
use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{InjectGlobalValue, RemoveComments, Rule};
use darklua_core::{
    process, BundleConfiguration, Configuration, GeneratorParameters, Options, Resources,
    WorkerTree,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::info;

const SOURCE_MAP_PREFIX: &str = "--@opacity-sourcemap ";

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct SourceMapFile {
    path: String,
    start: usize,
    end: usize,
}

fn relative_path(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn source_map_line(tree: &WorkerTree, resources: &Resources, cwd: &Path) -> Result<String> {
    let mut files = Vec::new();
    for range in tree
        .iter_bundle_source_ranges()
        .flat_map(|(_, ranges)| ranges)
    {
        let source_lines = resources
            .get(&range.path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {:?}", range.path.display(), e))?
            .lines()
            .count();
        files.push(SourceMapFile {
            path: relative_path(&range.path, cwd),
            start: range.start_line,
            end: range
                .end_line
                .min(range.start_line + source_lines.saturating_sub(1)),
        });
    }
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
    let source_map = source_map_line(&result, resources, &std::env::current_dir()?)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Platform, Settings};

    #[test]
    fn relative_path_strips_cwd() {
        let cwd = Path::new("/work/flows");
        assert_eq!(
            relative_path(Path::new("/work/flows/src/a.luau"), cwd),
            "src/a.luau"
        );
    }

    #[test]
    fn relative_path_keeps_paths_outside_cwd() {
        let cwd = Path::new("/work/flows");
        assert_eq!(
            relative_path(Path::new("/other/src/init.luau"), cwd),
            "/other/src/init.luau"
        );
    }

    #[test]
    fn relative_path_strips_dot_slash() {
        let cwd = Path::new("/work/flows");
        assert_eq!(relative_path(Path::new("./src/a.luau"), cwd), "src/a.luau");
    }

    #[test]
    fn relative_path_uses_forward_slashes() {
        let cwd = Path::new("/work/flows");
        assert_eq!(
            relative_path(Path::new("src\\sub\\a.luau"), cwd),
            "src/sub/a.luau"
        );
    }

    const ENTRY: &str = "local dep = require(\"./dep\")
-- a comment that the bundle drops
local function main()
	return dep.value() .. \"entry_line4\"
end
return main
";

    const DEP: &str = "local M = {}
-- another dropped comment
function M.value()
	return \"dep_line4\"
end
return M
";

    fn bundle_fixture() -> (String, Vec<SourceMapFile>, PathBuf) {
        let root = std::env::temp_dir().join(format!("opacity-cli-{}", uuid::Uuid::new_v4()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("entry.luau"), ENTRY).unwrap();
        std::fs::write(src.join("dep.luau"), DEP).unwrap();

        let flow = Flow {
            name: "Fixture".into(),
            alias: "fixture".into(),
            description: String::new(),
            min_sdk_version: None,
            retrieves: None,
            path: src.join("entry.luau").to_string_lossy().into_owned(),
        };
        let platform = Platform {
            name: "fixture".into(),
            description: String::new(),
            flows: vec![flow.clone()],
        };
        let config = Config {
            settings: Settings {
                output_directory: root.join("out").to_string_lossy().into_owned(),
                definition_files: None,
            },
            platforms: vec![platform],
        };
        let simple_platform = SimplePlatform::from(&config.platforms[0]);

        let options = create_options(&config, &simple_platform, &flow).unwrap();
        let output = options.output.clone();
        let resources = Resources::from_file_system();
        let result = process(&resources, options.opts).unwrap();
        assert!(result.collect_errors().is_empty());
        let bundle = std::fs::read_to_string(&output).unwrap();
        let source_map = source_map_line(&result, &resources, &root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        let map_json = source_map
            .trim_end()
            .strip_prefix(SOURCE_MAP_PREFIX)
            .expect("source map line must start with the prefix");
        let files: Vec<SourceMapFile> = serde_json::from_str(map_json).unwrap();
        (bundle, files, root)
    }

    #[test]
    fn source_map_points_every_source_line_at_its_bundle_line() {
        let (bundle, files, _) = bundle_fixture();
        let bundle_lines: Vec<&str> = bundle.lines().collect();
        let sources = [("src/dep.luau", DEP), ("src/entry.luau", ENTRY)];

        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            sources.iter().map(|(path, _)| *path).collect::<Vec<_>>()
        );

        for (file, (_, source)) in files.iter().zip(sources) {
            let source_lines: Vec<&str> = source.lines().collect();
            assert_eq!(file.end, file.start + source_lines.len() - 1, "{file:?}");

            for (index, line) in source_lines.iter().enumerate() {
                let actual = bundle_lines[file.start + index - 1];
                if line.contains("_line") {
                    assert!(
                        actual.contains(line.trim()),
                        "{file:?} line {index}: {actual}"
                    );
                }
                if line.starts_with("--") {
                    assert!(!actual.contains("--"), "{file:?} line {index}: {actual}");
                }
            }
        }
    }

    #[test]
    fn source_map_ranges_do_not_overlap() {
        let (_, files, _) = bundle_fixture();
        for pair in files.windows(2) {
            assert!(pair[0].end < pair[1].start, "{pair:?}");
        }
    }
}
