use std::collections::HashMap;

use anyhow::Result;
use full_moon::{tokenizer::TokenType, visitors::Visitor};

use crate::config::{Flow, Param, ParamVariant};

pub fn custom_process_bundle<'a>(flow: &'a mut Flow) -> Result<()> {
    let flow_source_code = std::fs::read_to_string(&flow.path)?;

    let mut ctx: Context = Context {
        flow_path: flow.path.clone(),
        param_variants: vec![],
        main_func: None,
        errors: vec![],
        all_types: HashMap::new(),
    };

    let mut visitor = ParamVisitor::new(&mut ctx);
    let ast = full_moon::parse(&flow_source_code)
        .map_err(|errs| {
            errs.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .map_err(|err| anyhow::anyhow!("Failed to parse flow: {}", err))?;

    println!("{}", serde_json::to_string_pretty(&ast).unwrap());
    // std::process::exit(0);
    visitor.visit_ast(&ast);

    // if there are any errors, just join by new line and return Err(joined_errs)
    if ctx.errors.len() > 0 {
        return Err(anyhow::anyhow!(
            "{}: \n{}",
            ctx.flow_path,
            ctx.errors
                .iter()
                .map(|e| format!("\t{}", e.to_string()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    if ctx.param_variants.len() > 0 {
        flow.params = Some(ctx.param_variants);
    } else {
        flow.params = None;
    }

    Ok(())
}

pub struct Context {
    pub param_variants: Vec<ParamVariant>,
    pub main_func: Option<full_moon::ast::FunctionDeclaration>,
    pub flow_path: String,
    pub errors: Vec<anyhow::Error>,
    pub all_types: HashMap<String, full_moon::ast::luau::TypeDeclaration>,
}

/// This visitor is used to traverse the AST and collect all the type declarations
/// and the main function's params argument
///
/// This works by first traversing the whole file's AST, collecting all types in a HashMap and the main function
///
/// After that, it tries to resolve the params argument if there is any
pub struct ParamVisitor<'a> {
    pub ctx: &'a mut Context,
}

impl<'a> ParamVisitor<'a> {
    pub fn new(ctx: &'a mut Context) -> Self {
        Self { ctx }
    }

    /// This function checks if the provided type is a simple primitive type: string, number, boolean, nil,
    pub fn is_simple_primitive_type(&self, tty: &full_moon::tokenizer::TokenType) -> bool {
        use full_moon::tokenizer::TokenType;
        match tty {
            TokenType::Identifier { identifier } => {
                identifier.to_string() == "string"
                    || identifier.to_string() == "number"
                    || identifier.to_string() == "boolean"
            }
            _ => false,
        }
    }

    /// Function used to resolve basic type of a value
    ///
    /// `type Params = { category: ->string<- }`
    fn resolve_basic_type(&self, basic: &full_moon::tokenizer::TokenReference) -> Result<String> {
        let ty = match basic.token_type() {
            TokenType::Identifier { identifier } => {
                if self.is_simple_primitive_type(basic.token_type()) {
                    identifier.to_string().trim().to_string()
                } else {
                    return Err(anyhow::anyhow!(
                        "Expected a simple primitive type, got a {:#?}",
                        basic.token_type()
                    ));
                }
            }
            TokenType::StringLiteral { .. } => {
                // TODO: maybe handle string literals?
                //       or add something to the description? I am not sure
                "string".to_string()
            }
            TokenType::Number { .. } => {
                // TODO: maybe handle number literals?
                //       or add something to the description? I am not sure
                "number".to_string()
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Expected a simple primitive type, got a {:#?}",
                    basic.token_type()
                ));
            }
        };

        Ok(ty)
    }

    /// This function resolves a table type to a Param
    /// You should provide a TypeInfo::Table
    pub fn resolve_table(&self, table: &full_moon::ast::luau::TypeInfo) -> Result<ParamVariant> {
        match table {
            full_moon::ast::luau::TypeInfo::Table { fields, .. } => {
                let mut param_variant: Vec<Param> = vec![];

                for field in fields {
                    let mut param = Param {
                        name: "".to_string(),
                        ty: "".to_string(),
                        required: true,
                        description: "".to_string(),
                    };
                    use full_moon::ast::luau::*;
                    let key = match field.key() {
                        TypeFieldKey::Name(name) => match name.token_type() {
                            TokenType::Identifier { identifier } => {
                                identifier.to_string().trim().to_string()
                            }
                            _ => {
                                return Err(anyhow::anyhow!(
                                    "Expected an identifier as the key, got a {:#?}",
                                    name.token_type()
                                ));
                            }
                        },
                        TypeFieldKey::IndexSignature { .. } => {
                            return Err(anyhow::anyhow!("Index signature is not supported yet: check the line containing '{}'", field.to_string()));
                        }
                        _ => unreachable!(),
                    };

                    let (value, required) = match field.value() {
                        TypeInfo::Basic(basic) => {
                            let ty = self.resolve_basic_type(basic)?;

                            (ty, false)
                        }
                        TypeInfo::Optional { base, .. } => match base.as_ref() {
                            TypeInfo::Basic(basic) => (self.resolve_basic_type(basic)?, true),
                            _ => {
                                return Err(anyhow::anyhow!(
                                    "Expected a simple primitive type, got a {:#?}",
                                    base.to_string()
                                ));
                            }
                        },
                        _ => Err(anyhow::anyhow!(
                            "Expected a simple primitive type, got a {:#?}",
                            field.value().to_string()
                        ))?,
                    };

                    param.name = key;
                    param.ty = value;
                    param.required = required;
                    // TODO: add a description to the param
                    param.description = "".to_string();

                    param_variant.push(param);
                }

                Ok(param_variant)
            }
            _ => Err(anyhow::anyhow!(
                "Expected a table, got a {}",
                table.to_string()
            )),
        }
    }

    /// Resolves the provided type info to a
    pub fn resolve_type_info(
        &self,
        ty: &full_moon::ast::luau::TypeInfo,
    ) -> Result<Vec<ParamVariant>> {
        use full_moon::ast::luau::*;
        fn create_err_string(s: &str) -> String {
            // format!("You can't have 'function main(params: {})', the 'params' argument has to be a table (or a union of a table and json null - Null). Either that or a simple Identifier pointing to a type declaration - such as 'function main(params: Params)'", s)
            format!("You can't have 'function main(params: {})', the 'params' argument has to be a table (params: {{category: string}}) or a simple Identifier pointing to a type declaration (params: Params). Also, only simple primitive types are supported for the fields of the table.", s)
        }
        match ty {
            TypeInfo::Array { .. } => Err(anyhow::anyhow!(create_err_string("array"))),
            TypeInfo::Basic(basic) => {
                match basic.token_type() {
                    TokenType::Identifier { identifier } => {
                        let type_def = self.ctx.all_types.get(&identifier.to_string()).ok_or(
                            anyhow::anyhow!("Type not found: {}", identifier.to_string()),
                        )?;
                        Ok(vec![self.resolve_table(type_def.type_definition())?])
                    }
                    _ => Err(anyhow::anyhow!(
                        "Expected an identifier, got a {:#?}",
                        basic.token_type()
                    )),
                }
            }
            TypeInfo::Boolean(..) => Err(anyhow::anyhow!(create_err_string("boolean"))),
            TypeInfo::Callback { .. } => Err(anyhow::anyhow!(create_err_string("callback"))),
            TypeInfo::Generic { .. } => Err(anyhow::anyhow!(create_err_string("generic"))),
            TypeInfo::GenericPack { .. } => Err(anyhow::anyhow!(create_err_string("generic pack"))),
            TypeInfo::Intersection(..) => {
                Err(anyhow::anyhow!(create_err_string(
                    "intersection - NOT YET IMPLEMENTED"
                )))
                // TODO: Implement intersection
            }
            TypeInfo::Module { .. } => Err(anyhow::anyhow!(create_err_string("module"))),
            TypeInfo::Optional { .. } => Err(anyhow::anyhow!(create_err_string("optional"))),
            TypeInfo::String(..) => Err(anyhow::anyhow!(create_err_string("string"))),
            TypeInfo::Table { .. } => Ok(vec![self.resolve_table(ty)?]),
            TypeInfo::Typeof { .. } => Err(anyhow::anyhow!(create_err_string("typeof"))),
            TypeInfo::Union(_ty_union) => {
                /*
                Case where we have

                type Action = {action: "start", start_arg: string} | {action: "stop", stop_arg: string}

                OR

                type Action = "start" | "stop"
                */
                Err(anyhow::anyhow!(create_err_string(
                    "union - NOT YET IMPLEMENTED"
                )))
                // TODO: Implement union
            }
            TypeInfo::Variadic { .. } => Err(anyhow::anyhow!(create_err_string("variadic"))),
            TypeInfo::VariadicPack { .. } => {
                Err(anyhow::anyhow!(create_err_string("variadic pack")))
            }
            _ => unreachable!(),
        }
    }

    /// First we have to collect all the types from the file, and only afterwards can we process the type of the main function's params argument
    pub fn resolve_main_function(&mut self) -> Result<()> {
        let func = match self.ctx.main_func.take() {
            Some(func) => func,
            None => {
                // this should be unreachable, user error
                println!("{}: No main function found", self.ctx.flow_path);
                self.ctx
                    .errors
                    .push(anyhow::anyhow!("No main function found"));
                return Ok(());
            }
        };

        // we are in the main function
        // first we check if we have a params argument in the main function
        let body = func.body();

        let no_of_params = body.parameters().len();

        if no_of_params == 0 {
            // no params, so we can return
            return Ok(());
        }

        if no_of_params > 1 {
            println!(
                "{}: Multiple params in main function are not supported yet",
                self.ctx.flow_path
            );
            self.ctx.errors.push(anyhow::anyhow!(
                "Multiple params in main function are not supported yet"
            ));
        }

        // next, we get its type, if it has any associated with it
        let ty = body.type_specifiers().next();
        if let Some(ty) = ty {
            if let Some(ty) = ty {
                match self.resolve_type_info(ty.type_info()) {
                    Ok(ty) => {
                        self.ctx.param_variants = ty;
                    }
                    Err(e) => {
                        println!("{}: {}", self.ctx.flow_path, e);
                        self.ctx.errors.push(e);
                    }
                }
            } else {
                // tell user
                println!(
                    "{}: No type associated with main function's params argument",
                    self.ctx.flow_path
                );
                self.ctx.errors.push(anyhow::anyhow!(
                    "No type associated with main function's params argument"
                ));
            }
        } else {
            // we couldn't find any associated type, tell user
            println!(
                "{}: No type associated with main function's params argument",
                self.ctx.flow_path
            );
            self.ctx.errors.push(anyhow::anyhow!(
                "No type associated with main function's params argument"
            ));
        }

        Ok(())
    }
}

impl<'a> full_moon::visitors::Visitor for ParamVisitor<'a> {
    /**
     * function main(params: Params)
     * OR
     * function main(params: {param1: blabla, param2: blabla})
     */
    fn visit_ast(&mut self, ast: &full_moon::ast::Ast)
    where
        Self: Sized,
    {
        use full_moon::ast::*;
        // find the main function declaration
        for node in ast.nodes().stmts() {
            match node {
                Stmt::FunctionDeclaration(func) => {
                    let name_pair = match func.name().names().first() {
                        Some(name) => name,
                        None => continue,
                    };

                    if let TokenType::Identifier { identifier: s } = name_pair.value().token_type()
                    {
                        if s.trim() != "main" {
                            continue;
                        }
                    } else {
                        continue;
                    }

                    // we are in the main function, so let's save it
                    self.ctx.main_func = Some(func.clone());
                }
                Stmt::TypeDeclaration(ty) => {
                    let name = ty.type_name().token_type();
                    if let TokenType::Identifier { identifier } = name {
                        self.ctx
                            .all_types
                            .insert(identifier.to_string(), ty.clone());
                    }
                }
                _ => {}
            }
        }

        // we've traversed the entire file and collected all the type declarations
        // now we can try and resolve the main function's params argument
        if let Err(e) = self.resolve_main_function() {
            println!("{}: {}", self.ctx.flow_path, e);
            self.ctx.errors.push(e);
        }
    }
}
