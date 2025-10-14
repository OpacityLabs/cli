use opacity_cli::commands::bundle::param_extractor::{self, Param};

#[test]
pub fn test_t9() {
    let file_path = std::path::Path::new(std::env::current_dir().unwrap().as_os_str())
        .join("tests/params_extraction//t9/flow.luau");
    let file = std::fs::read_to_string(file_path).unwrap();

    let params = param_extractor::extract_params(&file, "flow.luau", None).unwrap();

    assert_eq!(
        params,
        vec![vec![
            Param { 
                name: "a".to_string(),
                description: "this is a comment about the field a".to_string(),
                ty: "number".to_string(),
                required: true,
            },
            Param { 
                name: "b".to_string(),
                description: "this is a single comment about the field b\nthis is a secondary single comment about the field b".to_string(),
                ty: "number".to_string(),
                required: true,
            },
            Param { 
                name: "c".to_string(),
                description: "This is a multiline comment\nabout the field c".to_string(),
                ty: "number".to_string(),
                required: true,
            },
        ]]
    )
}
