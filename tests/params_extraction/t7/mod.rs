use opacity_cli::commands::bundle::param_extractor::{self, Param};

#[test]
pub fn test_t7() {
    let file_path = std::path::Path::new(std::env::current_dir().unwrap().as_os_str())
        .join("tests/params_extraction//t7/flow.luau");
    let file = std::fs::read_to_string(file_path).unwrap();

    let params = param_extractor::extract_params(&file, "flow.luau", Some("tests/params_extraction//t7/".to_string())).unwrap();

    assert_eq!(
        params,
        vec![
            vec![Param {
                name: "action".to_string(),
                description: "".to_string(),
                ty: "\"action1\"".to_string(),
                required: true,
            },],
            vec![Param {
                name: "action".to_string(),
                description: "".to_string(),
                ty: "\"action2\"".to_string(),
                required: true,
            },]
        ]
    )
}
