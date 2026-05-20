use crate::commands::version::compute_versions;
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
use tracing::info;

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

const HASHES_FILE_NAME: &'static str = "hashes.lock";

fn update_hashes(hashes_file_path: &PathBuf, new_hashes: &[(String, String)]) -> Result<()> {
    let existing = std::fs::read_to_string(hashes_file_path)?;
    // parse
    let mut entries: Vec<(String, String)> = existing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            line.rsplit_once(':')
                .map(|(path, hash)| (path.to_string(), hash.to_string()))
        })
        .collect();

    for (path, hash) in new_hashes {
        match entries.iter_mut().find(|(p, _)| p == path) {
            Some(entry) => entry.1 = hash.clone(),
            None => {
                let pos = entries
                    .binary_search_by(|(p, _)| p.as_str().cmp(path.as_str()))
                    .unwrap_or_else(|i| i);
                entries.insert(pos, (path.clone(), hash.clone()));
            }
        }
    }

    // generate code
    let content = entries
        .iter()
        .map(|(path, hash)| format!("{}:{}", path, hash))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(hashes_file_path, content)?;
    Ok(())
}

pub fn bundle(config_path: &str, is_rebundle: bool, file_to_bundle: Option<&str>) -> Result<()> {
    let config_path_dir_buf = {
        let mut temp_path = PathBuf::from(config_path);
        temp_path.pop();
        temp_path
    };
    let hashes_file_path = config_path_dir_buf.join(HASHES_FILE_NAME);
    if !std::fs::exists(&hashes_file_path)? {
        std::fs::write(&hashes_file_path, "")?;
    }
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    std::fs::create_dir_all(&config.settings.output_directory)?;

    let mut file_paths: Vec<PathBuf> = Vec::new();

    match file_to_bundle {
        None => {
            for platform in &config.platforms {
                println!("Processing platform: {}", platform.name);
                let simple_platform = SimplePlatform::from(platform);

                for flow in &platform.flows {
                    println!("Bundling {} ({})", flow.name, flow.alias);

                    let bundle_options = create_options(&config, &simple_platform, flow)?;

                    file_paths.push(bundle_options.output.clone());

                    process_bundle(&resources, bundle_options.opts)?;
                }
            }
        }
        Some(file_to_bundle) => {
            let matched = config.platforms.iter().find_map(|platform| {
                platform
                    .flows
                    .iter()
                    .find(|flow| flow.alias == file_to_bundle)
                    .map(|flow| (platform, flow))
            });

            match matched {
                Some((platform, flow)) => {
                    println!("Processing platform: {}", platform.name);
                    let simple_platform = SimplePlatform::from(platform);

                    println!("Bundling {} ({})", flow.name, flow.alias);
                    let bundle_options = create_options(&config, &simple_platform, flow)?;
                    file_paths.push(bundle_options.output.clone());
                    process_bundle(&resources, bundle_options.opts)?;
                }
                None => {
                    anyhow::bail!(
                        "File '{}' does not match any flow.path in config",
                        file_to_bundle
                    );
                }
            }
        }
    }

    let hashes = compute_hashes(&mut file_paths)?;

    update_hashes(&hashes_file_path, &hashes)?;

    compute_versions(config_path)?;

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
    use std::fs;
    use tempfile::TempDir;

    fn read_lines(path: &PathBuf) -> Vec<String> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn make_flow(name: &str, alias: &str, path: &str) -> Flow {
        Flow {
            name: name.to_string(),
            alias: alias.to_string(),
            description: String::new(),
            min_sdk_version: None,
            retrieves: None,
            path: path.to_string(),
        }
    }

    fn make_platform() -> SimplePlatform {
        SimplePlatform {
            name: "ios".to_string(),
            description: "iOS platform".to_string(),
        }
    }

    // ---------- update_hashes ----------

    #[test]
    fn update_hashes_inserts_into_empty_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("hashes.lock");
        fs::write(&path, "").unwrap();

        let new = vec![
            ("a.luau".to_string(), "h1".to_string()),
            ("b.luau".to_string(), "h2".to_string()),
        ];
        update_hashes(&path, &new).unwrap();

        assert_eq!(read_lines(&path), vec!["a.luau:h1", "b.luau:h2"]);
    }

    #[test]
    fn update_hashes_updates_in_place_preserving_position() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("hashes.lock");
        fs::write(&path, "a.luau:old1\nb.luau:old2\nc.luau:old3").unwrap();

        // update only b — a and c stay, b's hash changes, order preserved
        let new = vec![("b.luau".to_string(), "new2".to_string())];
        update_hashes(&path, &new).unwrap();

        assert_eq!(
            read_lines(&path),
            vec!["a.luau:old1", "b.luau:new2", "c.luau:old3"]
        );
    }

    #[test]
    fn update_hashes_inserts_new_entry_at_alphabetical_position() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("hashes.lock");
        fs::write(&path, "a.luau:h1\nc.luau:h3").unwrap();

        // b should land between a and c
        let new = vec![("b.luau".to_string(), "h2".to_string())];
        update_hashes(&path, &new).unwrap();

        assert_eq!(
            read_lines(&path),
            vec!["a.luau:h1", "b.luau:h2", "c.luau:h3"]
        );
    }

    #[test]
    fn update_hashes_mix_of_updates_and_inserts() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("hashes.lock");
        fs::write(&path, "a.luau:old_a\nc.luau:old_c\ne.luau:old_e").unwrap();

        // update a, insert b (between a,c), update c, insert d (between c,e), insert z (end)
        let new = vec![
            ("a.luau".to_string(), "new_a".to_string()),
            ("b.luau".to_string(), "new_b".to_string()),
            ("c.luau".to_string(), "new_c".to_string()),
            ("d.luau".to_string(), "new_d".to_string()),
            ("z.luau".to_string(), "new_z".to_string()),
        ];
        update_hashes(&path, &new).unwrap();

        assert_eq!(
            read_lines(&path),
            vec![
                "a.luau:new_a",
                "b.luau:new_b",
                "c.luau:new_c",
                "d.luau:new_d",
                "e.luau:old_e",
                "z.luau:new_z",
            ]
        );
    }

    #[test]
    fn update_hashes_handles_blank_lines_in_existing_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("hashes.lock");
        fs::write(&path, "\n\na.luau:h1\n\n").unwrap();

        let new = vec![("b.luau".to_string(), "h2".to_string())];
        update_hashes(&path, &new).unwrap();

        assert_eq!(read_lines(&path), vec!["a.luau:h1", "b.luau:h2"]);
    }

    // ---------- compute_hashes ----------

    #[test]
    fn compute_hashes_returns_known_sha256() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        fs::write(&file, "hello").unwrap();

        let mut paths = vec![file.clone()];
        let result = compute_hashes(&mut paths).unwrap();

        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].1,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(result[0].0, file.to_string_lossy().to_string());
    }

    #[test]
    fn compute_hashes_sorts_paths_alphabetically() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        let c = tmp.path().join("c.txt");
        fs::write(&a, "A").unwrap();
        fs::write(&b, "B").unwrap();
        fs::write(&c, "C").unwrap();

        // pass in scrambled order
        let mut paths = vec![c.clone(), a.clone(), b.clone()];
        let result = compute_hashes(&mut paths).unwrap();

        let returned: Vec<String> = result.iter().map(|(p, _)| p.clone()).collect();
        let mut expected = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
            c.to_string_lossy().to_string(),
        ];
        expected.sort();
        assert_eq!(returned, expected);
    }

    #[test]
    fn compute_hashes_fails_on_missing_file() {
        let tmp = TempDir::new().unwrap();
        let mut paths = vec![tmp.path().join("does-not-exist.txt")];
        assert!(compute_hashes(&mut paths).is_err());
    }

    // ---------- get_global_inject_rules ----------

    #[test]
    fn inject_rules_minimum_count_without_optional_fields() {
        let platform = make_platform();
        let flow = make_flow("flow_a", "fa", "flows/a.luau");
        let rules = get_global_inject_rules(&platform, &flow);
        // FLOW_NAME, FLOW_ALIAS, PLATFORM_NAME, PLATFORM_DESCRIPTION
        assert_eq!(rules.len(), 4);
    }

    #[test]
    fn inject_rules_includes_min_sdk_version_when_set() {
        let platform = make_platform();
        let mut flow = make_flow("flow_a", "fa", "flows/a.luau");
        flow.min_sdk_version = Some("1.2.3".to_string());
        let rules = get_global_inject_rules(&platform, &flow);
        assert_eq!(rules.len(), 5);
    }

    #[test]
    fn inject_rules_includes_retrieves_when_set() {
        let platform = make_platform();
        let mut flow = make_flow("flow_a", "fa", "flows/a.luau");
        flow.retrieves = Some(vec!["x".to_string(), "y".to_string()]);
        let rules = get_global_inject_rules(&platform, &flow);
        assert_eq!(rules.len(), 5);
    }

    #[test]
    fn inject_rules_with_all_optional_fields() {
        let platform = make_platform();
        let mut flow = make_flow("flow_a", "fa", "flows/a.luau");
        flow.min_sdk_version = Some("1.0".to_string());
        flow.retrieves = Some(vec!["x".to_string()]);
        let rules = get_global_inject_rules(&platform, &flow);
        assert_eq!(rules.len(), 6);
    }

    // ---------- bundle (integration) ----------

    fn setup_project(tmp: &TempDir, output_subdir: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = tmp.path();
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let flow_a = src_dir.join("flow_a.luau");
        let flow_b = src_dir.join("flow_b.luau");
        fs::write(&flow_a, "return { name = \"a\" }\n").unwrap();
        fs::write(&flow_b, "return { name = \"b\" }\n").unwrap();

        let output_dir = root.join(output_subdir);

        // minimal version_file.json — compute_versions reads this next to opacity.toml
        let version_file = root.join("version_file.json");
        fs::write(
            &version_file,
            r#"{"defaultVersion":1,"functionMappings":{},"sdkVersionFunction":"fetch_sdk_version"}"#,
        )
        .unwrap();

        let config_path = root.join("opacity.toml");
        let toml = format!(
            r#"[settings]
output_directory = "{output}"

[[platforms]]
name = "Test"
description = "Test platform"
flows = [
    {{ name = "Flow A", alias = "test_flow_a", description = "A", minSdkVersion = "1", retrieves = ["data_a"], path = "{flow_a}" }},
    {{ name = "Flow B", alias = "test_flow_b", description = "B", minSdkVersion = "1", retrieves = ["data_b"], path = "{flow_b}" }},
]
"#,
            output = output_dir.display(),
            flow_a = flow_a.display(),
            flow_b = flow_b.display(),
        );
        fs::write(&config_path, toml).unwrap();

        (config_path, output_dir, root.to_path_buf())
    }

    #[test]
    fn bundle_all_flows_creates_outputs_and_hashes() {
        let tmp = TempDir::new().unwrap();
        let (config_path, output_dir, root) = setup_project(&tmp, "bundled_all");

        bundle(config_path.to_str().unwrap(), false, None).unwrap();

        let out_a = output_dir.join("test_flow_a.bundle.luau");
        let out_b = output_dir.join("test_flow_b.bundle.luau");
        assert!(out_a.exists(), "expected bundle output: {:?}", out_a);
        assert!(out_b.exists(), "expected bundle output: {:?}", out_b);

        let hashes_path = root.join("hashes.lock");
        assert!(hashes_path.exists());
        let hashes = fs::read_to_string(&hashes_path).unwrap();
        let lines: Vec<&str> = hashes.lines().collect();
        assert_eq!(lines.len(), 2);
        // alphabetical order by path
        assert!(lines[0].contains("test_flow_a.bundle.luau"));
        assert!(lines[1].contains("test_flow_b.bundle.luau"));
        for line in &lines {
            let (_, hash) = line.rsplit_once(':').unwrap();
            assert_eq!(hash.len(), 64, "sha256 hex should be 64 chars: {}", hash);
        }

        assert!(root.join("versions.lock").exists());
    }

    #[test]
    fn bundle_single_flow_only_updates_matching_entry() {
        let tmp = TempDir::new().unwrap();
        let (config_path, output_dir, root) = setup_project(&tmp, "bundled_single");

        // first bundle everything to seed hashes.lock
        bundle(config_path.to_str().unwrap(), false, None).unwrap();
        let hashes_before = fs::read_to_string(root.join("hashes.lock")).unwrap();

        // modify flow_b source, re-bundle only flow_b by alias
        let flow_b_src = root.join("src/flow_b.luau");
        fs::write(&flow_b_src, "return { name = \"b\", changed = true }\n").unwrap();

        bundle(config_path.to_str().unwrap(), true, Some("test_flow_b")).unwrap();

        let hashes_after = fs::read_to_string(root.join("hashes.lock")).unwrap();
        let lines_after: Vec<&str> = hashes_after.lines().collect();
        assert_eq!(lines_after.len(), 2, "should still have 2 entries");

        // both bundle output files still exist
        assert!(output_dir.join("test_flow_a.bundle.luau").exists());
        assert!(output_dir.join("test_flow_b.bundle.luau").exists());

        // flow_a hash unchanged, flow_b hash changed
        let line_a_before = hashes_before
            .lines()
            .find(|l| l.contains("test_flow_a"))
            .unwrap();
        let line_a_after = hashes_after
            .lines()
            .find(|l| l.contains("test_flow_a"))
            .unwrap();
        assert_eq!(line_a_before, line_a_after, "flow_a should be untouched");

        let line_b_before = hashes_before
            .lines()
            .find(|l| l.contains("test_flow_b"))
            .unwrap();
        let line_b_after = hashes_after
            .lines()
            .find(|l| l.contains("test_flow_b"))
            .unwrap();
        assert_ne!(line_b_before, line_b_after, "flow_b hash should change");
    }

    #[test]
    fn bundle_single_flow_bails_when_alias_does_not_match_config() {
        let tmp = TempDir::new().unwrap();
        let (config_path, _output_dir, _root) = setup_project(&tmp, "bundled_bail");

        let result = bundle(
            config_path.to_str().unwrap(),
            false,
            Some("test_nonexistent_alias"),
        );
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("does not match any flow"),
            "unexpected error message: {}",
            msg
        );
    }
}
