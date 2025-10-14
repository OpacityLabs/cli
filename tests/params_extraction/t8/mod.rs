use opacity_cli::commands::bundle::param_extractor::{self, Param};

#[test]
pub fn test_t8() {
    let file_path = std::path::Path::new(std::env::current_dir().unwrap().as_os_str())
        .join("tests/params_extraction//t8/flow.luau");
    let file = std::fs::read_to_string(file_path).unwrap();

    let params = param_extractor::extract_params(&file, "flow.luau", None).unwrap();

    assert_eq!(
        params,
        vec![vec![
            Param { 
                name: "field1".to_string(),
                description: "".to_string(),
                ty: "string".to_string(),
                required: false,
            },
            Param { 
                name: "field2".to_string(),
                description: "".to_string(),
                ty: "string".to_string(),
                required: true,
            },
            Param { 
                name: "field999".to_string(),
                description: "".to_string(),
                ty: "\"A\"".to_string(),
                required: true,
            },
            Param { 
                name: "field998".to_string(),
                description: "".to_string(),
                ty: "\"A\" | \"B\" | \"C\"".to_string(),
                required: true,
            },
            Param { 
                name: "field3".to_string(),
                description: "".to_string(),
                ty: "\"A\" | \"B\" | \"C\"".to_string(),
                required: false,
            },
            Param { 
                name: "field4".to_string(),
                description: "".to_string(),
                ty: "number".to_string(),
                required: false,
            },
            Param { 
            name: "field5".to_string(),
                description: "".to_string(),
                ty: "number".to_string(),
                required: true,
            },
            Param { 
                name: "field6".to_string(),
                description: "".to_string(),
                ty: "boolean".to_string(),
                required: false,
            },
            Param { 
                name: "field7".to_string(),
                description: "".to_string(),
                ty: "boolean".to_string(),
                required: true,
            },
            Param { 
                name: "field8".to_string(),
                description: "".to_string(),
                ty: "false".to_string(),
                required: true,
            },
            Param { 
                name: "field9".to_string(),
                description: "".to_string(),
                ty: "true".to_string(),
                required: true,
            },
            Param { 
                name: "field10".to_string(),
                description: "".to_string(),
                ty: "\"A\" | \"B\" | \"C\"".to_string(),
                required: false,
            },
        ]]
    )
}
