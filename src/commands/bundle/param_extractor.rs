use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};

use darklua_core::{
    nodes::{
        Block, FunctionStatement, Statement, TableEntryType, TableType,
        TriviaKind, Type, TypeDeclarationStatement,
    },
    process::NodeProcessor,
    Parser,
};
use serde::{Deserialize, Serialize};

type ParamType = String;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub description: String,
    // we keep it as a String for now
    // maybe we move to a String | Table later :)
    pub ty: ParamType,
    pub required: bool,
}

impl Param {
	pub fn to_toml_table(&self) -> toml_edit::Table {
			let mut table = toml_edit::Table::new();
			table.insert("name", self.name.clone().into());
			table.insert("description", self.description.clone().into());
			table.insert("ty", self.ty.clone().into());
			table.insert("required", self.required.into());
			table
	}
}

pub type ParamVariant = Vec<Param>;

#[derive(Debug, PartialEq, Eq)]
pub struct Module {
    pub local_types: HashMap<String, TypeDeclarationStatement>,
    pub source_code: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ModuleEnum {
    Resolved(Module),
    NotYetResolved,
}

type ModulePath = String;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Context {
    cwd: Option<String>,
    are_we_in_main_block: bool,
    errors: Vec<String>,
    params: Vec<ParamVariant>,
    main_function: Option<FunctionStatement>,
    /// HashMap that keeps track of the file's path to the module enum
    modules: HashMap<ModulePath, ModuleEnum>,
    name_to_module_path: HashMap<String, ModulePath>,
    main_module_types: HashMap<String, TypeDeclarationStatement>,
    main_module_path: String,
    /// Funny thing: this is "" (empty) for the main module
    current_module_path: String,
    main_module_source_code: String,
}

pub struct ParamExtractorVisitor(pub Context);

impl ParamExtractorVisitor {
    pub fn new(cwd: Option<String>) -> Self {
        Self(Context {
            are_we_in_main_block: true,
            cwd,
            ..Default::default()
        })
    }

    fn get_string_comment(&self, token: &darklua_core::nodes::Token) -> String {
        token
            .iter_leading_trivia()
            .filter(|t| matches!(t.kind(), TriviaKind::Comment))
            .map(|t| {
                fn is_multiline_comment(comment: &str) -> bool {
                    comment.starts_with("--[[") && comment.ends_with("]]")
                }

                let read_value = t.read(self.get_current_file_source_code()).to_owned();

                match is_multiline_comment(&read_value) {
                    true => read_value[4..read_value.len() - 2]
                        .split("\n")
                        .map(|line| line.trim())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    false => read_value[2..].trim().to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn get_current_file_source_code(&self) -> &str {
        if self.0.current_module_path.is_empty() {
            return self.0.main_module_source_code.as_str();
        }

        let module = self.0.modules.get(&self.0.current_module_path).unwrap();
        match module {
            ModuleEnum::Resolved(module) => module.source_code.as_str(),
            ModuleEnum::NotYetResolved => unreachable!(),
        }
    }

    /// Function used to get the type declaration for a name depending on the current module path (which should be passed just to be extra sure)
    fn get_type_decl_for_name(
        &mut self,
        curr_module_path: String,
        name: String,
    ) -> Result<TypeDeclarationStatement, anyhow::Error> {
        match curr_module_path.as_str() {
            "" => {
                // we are in the main module
                Ok(self
                    .0
                    .main_module_types
                    .get(name.as_str())
                    .ok_or(anyhow::anyhow!(
                        "Type `{}` not found in main module types",
                        name
                    ))?
                    .clone())
            }
            module_path => {
                let module = self
                    .0
                    .modules
                    .get(module_path)
                    .ok_or(anyhow::anyhow!("Module not found: {}", module_path))?;
                match module {
                    ModuleEnum::Resolved(module) => Ok(module
                        .local_types
                        .get(name.as_str())
                        .ok_or(anyhow::anyhow!("Type `{}` not found in module types", name))?
                        .clone()),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// This is used for fields of our table
    /// We only accept simple types, no user defined types
    /// We return the mapped type from lua to our types
    fn resolve_simple_type(&mut self, ty: &Type) -> Result<String, anyhow::Error> {
        Ok(match ty {
            Type::False(_) => "false".to_string(),
            Type::Name(name) => {
                // primitive types or user defined types
                // for now only accept simple types, no user defined types
                match name.get_type_name().get_name().to_string().as_str() {
                    "string" => "string".to_string(),
                    "number" => "number".to_string(),
                    "boolean" => "boolean".to_string(),
                    // TODO: add support for other primitive types
                    _ => {
                        if !Self::is_user_defined_type(
                            name.get_type_name().get_name().to_string().as_str(),
                        ) {
                            Err(anyhow::anyhow!(
															"Unsupported type: '{}'. Only string, number and boolean are supported as primitive types",
															name.get_type_name().get_name().to_string()
													))?
                        }

                        let name_str = name.get_type_name().get_name();

                        let type_decl = self.get_type_decl_for_name(
                            self.0.current_module_path.to_owned(),
                            name_str.to_owned(),
                        )?;

                        // for now, if you are going to hide the actual field type behind a user-defined type,
                        // you have to make sure that that is a union of string literals, it's the only accepted type

                        let mut tys = Vec::new();
                        match type_decl.get_type() {
                            Type::Union(union) => {
                                for ty in union.iter_types() {
                                    match ty {
                                        Type::String(string) => {
                                            tys.push(format!(
                                                "\"{}\"",
                                                String::from_utf8_lossy(string.get_value())
                                            ));
                                        }
                                        _ => Err(anyhow::anyhow!(
                                            "Unsupported type declaration: {:?}",
                                            type_decl.get_type()
                                        ))?,
                                    }
                                }
                                tys.join(" | ")
                            }
                            _ => Err(anyhow::anyhow!(
                                "Unsupported type declaration: {:?}",
                                type_decl.get_type()
                            ))?,
                        }
                    }
                }
            }
            Type::String(string) => format!("\"{}\"", String::from_utf8_lossy(string.get_value())),
            Type::True(_) => "true".to_string(),
            Type::Optional(optional) => self.resolve_simple_type(optional.get_inner_type())?,
            _ => Err(anyhow::anyhow!("Unsupported simple type: {:?}", ty))?,
        })
    }

    /// This is used for simple union types like `ActionType = "start" | "status" | "download"`
    /// We only accept the string simple type for now
    fn resolve_simple_union_type(&self, ty: &Type) -> Result<String, anyhow::Error> {
        Ok(match ty {
            Type::Union(union) => {
                let mut tys: Vec<String> = Vec::new();
                for ty in union.iter_types() {
                    match ty {
                        Type::String(string) => {
                            tys.push(format!(
                                "\"{}\"",
                                String::from_utf8_lossy(string.get_value())
                            ));
                        }
                        _ => Err(anyhow::anyhow!("Unsupported simple union type: {:?}", ty))?,
                    }
                }

                tys.join(" | ")
            }
            _ => Err(anyhow::anyhow!("Expected a union type, got a {:?}", ty))?,
        })
    }

    // this resolves a union type that is the type of a field in a type table
    fn resolve_union_type(&self, ty: &Type) -> Result<String, anyhow::Error> {
        Ok(match ty {
            Type::Union(union) => {
                let mut tys = Vec::new();

                for ty in union.iter_types() {
                    // in
                    match ty {
                        Type::String(string) => {
                            tys.push(format!(
                                "\"{}\"",
                                String::from_utf8_lossy(string.get_value())
                            ));
                        }
                        Type::Field(_) => Err(anyhow::anyhow!(
                            "Field types are not supported yet inside union types"
                        ))?,
                        Type::Name(_) => Err(anyhow::anyhow!(
                            "Name types are not supported yet inside union types"
                        ))?,
                        Type::Optional(optional) => {
                            // we do accept "A"? | "C"?
                            // TODO: maybe we also have to set the param type here as optional?
                            match optional.get_inner_type() {
                                Type::String(string) => {
                                    tys.push(format!(
                                        "\"{}\"",
                                        String::from_utf8_lossy(string.get_value())
                                    ));
                                }
                                _ => Err(anyhow::anyhow!(
                                    "Unsupported optional type inside union types: {:?}",
                                    optional.get_inner_type()
                                ))?,
                            }
                        }
                        Type::Nil(_) => {
                            todo!()
                        }
                        _ => Err(anyhow::anyhow!("Unsupported union type: {:?}", ty))?,
                    }
                }

                tys.join(" | ")
            }
            _ => Err(anyhow::anyhow!("Expected a union type, got a {:?}", ty))?,
        })
    }

    /// If we have something like:
    ///
    /// ```luau
    /// type Params = {a: string?, b: number?}
    /// ```
    ///
    /// or even
    ///
    /// ```luau
    /// type State = {a: string?, b: number?}
    /// type Params = State
    /// ```
    ///
    /// it resolves the actual table
    fn resolve_type_table(
        &mut self,
        type_table: &TableType,
    ) -> Result<ParamVariant, anyhow::Error> {
        let mut params = ParamVariant::new();

        /*
        We currently accept fields like this:
        field1: string?,
        field2: string,
        field999: "A",
        field3: "A" | "B" | "C",
        field4: number?,
        field5: number,
        field6: boolean?,
        field7: boolean,
                        field8: false,
                        field9: true,
         */
        for entry in type_table.iter_entries() {
            let mut curr_param = Param {
                name: String::new(),
                description: String::new(),
                ty: String::new(),
                required: true,
            };

            let value = match entry {
                TableEntryType::Property(prop) => {
                    curr_param.name = prop.get_identifier().get_name().to_string();
                    let comment = match prop.get_identifier().get_token() {
                        Some(token) => self.get_string_comment(token),
                        None => String::new(),
                    };
                    curr_param.description = comment;
                    prop.get_type()
                },
								// Some property names are NOT valid, for example "end", which is a reserved keyword
								// You might want to have both "start" and "end" as simple properties, but you can't
								// The solution is to have a literal property, which is a string literal
								TableEntryType::Literal(literal) => {
									curr_param.name = String::from_utf8_lossy(literal.get_string().get_value()).to_string();
									let comment = match literal.get_string().get_token() {
										Some(token) => self.get_string_comment(token),
										None => String::new(),
									};
									curr_param.description = comment;
									literal.get_type()
								}
                _ => return Err(anyhow::anyhow!("Expected a property, got a {:?}", entry)),
            };

						curr_param.ty = match value {
							Type::Array(array) => {
									// this can either be a simple primitive type or a union of strings
									let ty = match array.get_element_type() {
											Type::False(_)
											| Type::Name(_)
											| Type::String(_)
											| Type::True(_) => self.resolve_simple_type(value)?,
											Type::Union(_) => self.resolve_simple_union_type(value)?,
											_ => Err(anyhow::anyhow!(
													"Unsupported array type: {:?}",
													array.get_element_type()
											))?,
									};

									format!("Vec<{ty}>")
							}
							Type::False(_) | Type::Name(_) | Type::String(_) | Type::True(_) => {
									self.resolve_simple_type(value)?
							}
							Type::Field(_) => {
									Err(anyhow::anyhow!("Field types are not supported yet"))?
							}
							Type::Function(_) => {
									Err(anyhow::anyhow!("Function types are not supported"))?
							}
							Type::Intersection(_) => {
									Err(anyhow::anyhow!("Intersection types are not supported"))?
							}
							Type::Nil(_) => Err(anyhow::anyhow!("Nil types are not supported yet"))?,
							Type::Optional(optional) => {
									curr_param.required = false;
									// this should be like the above, simple types
									// what it can also be is a paranthese type, ughh
									match optional.get_inner_type() {
											Type::False(_)
											| Type::Name(_)
											| Type::String(_)
											| Type::True(_) => self.resolve_simple_type(value)?,
											Type::Union(_) => self.resolve_union_type(value)?,
											Type::Parenthese(parenthese) => {
													let value = parenthese.get_inner_type();
													// TODO: maybe wrap final value in parentheses? maybe not?
													match value {
															Type::Union(_) => self.resolve_union_type(value)?,
															_ => Err(anyhow::anyhow!(
																"Unsupported optional type inside paranthese types: {:?}",
																parenthese.get_inner_type()
														))?,
													}
											}
											_ => Err(anyhow::anyhow!(
													"Unsupported optional type: {:?}",
													optional.get_inner_type()
											))?,
									}
							}
							Type::Parenthese(parenthese) => {
									// this should be like the above, simple types
									let ty = match parenthese.get_inner_type() {
											Type::False(_)
											| Type::Name(_)
											| Type::String(_)
											| Type::True(_) => self.resolve_simple_type(value)?,
											Type::Union(_) => self.resolve_union_type(value)?,
											_ => Err(anyhow::anyhow!(
													"Unsupported paranthese type: {:?}",
													parenthese
											))?,
									};

									format!("({ty})")
							}
							Type::Table(_) => {
									Err(anyhow::anyhow!("Table types are not supported yet"))?
							}
							Type::TypeOf(_) => Err(anyhow::anyhow!("Type of types are not supported"))?,
							Type::Union(_) => self.resolve_union_type(value)?,
					};

            params.push(curr_param);
        }

        Ok(params)
    }

    /// this is the resolver that the [[Self::resolve_params_type_decl]] uses
    /// because we might have a union `type Param = State1 | State2`, and we'd want to delegate
    fn resolve_type_decl_for_param_variant(
        &mut self,
        type_decl: &Type,
    ) -> Result<ParamVariant, anyhow::Error> {
        match type_decl {
					Type::Table(table) => self.resolve_type_table(table),
					Type::Field(_) => Err(anyhow::anyhow!(
							"External field types are not supported yet, please have both the main Params type and the types for your fields in the same file"
					))?,
					Type::Name(name) => {
							let name_str = name.get_type_name().get_name().to_string();
							let type_decl = self.get_type_decl_for_name(
									self.0.current_module_path.to_owned(),
									name_str.to_owned(),
							)?;

							match type_decl.get_type() {
									Type::Table(table) => self.resolve_type_table(table),
									_ => Err(anyhow::anyhow!(
											"Unsupported type declaration: {:?}",
											type_decl.get_type()
									))?,
							}
					}
					_ => Err(anyhow::anyhow!(
							"Unsupported type declaration: {:?}",
							type_decl
					))?,
			}
    }

    /// this is the resolver for the main `type Params = ...`
    fn resolve_params_type_decl(
        &mut self,
        type_decl: &TypeDeclarationStatement,
    ) -> Result<Vec<ParamVariant>, anyhow::Error> {
        let mut param_variants: Vec<ParamVariant> = Vec::new();

        match type_decl.get_type() {
            // [[Type::Field(_)]] is like an external type requiredModule.TypeInsideIt
            Type::Table(_) | Type::Name(_) => {
                param_variants.push(self.resolve_type_decl_for_param_variant(type_decl.get_type())?)
            }
            Type::Field(field) => {
                let prop_name = field.get_type_name().get_type_name().get_name().to_string();

                let type_decl = self.get_type_decl_for_name(
                    self.0.current_module_path.to_owned(),
                    prop_name.to_owned(),
                )?;

                match type_decl.get_type() {
                    Type::Union(union) => {
                        for ty in union.iter_types() {
                            param_variants.push(self.resolve_type_decl_for_param_variant(ty)?)
                        }
                    }
                    _ => param_variants
                        .push(self.resolve_type_decl_for_param_variant(type_decl.get_type())?),
                }
            }

            Type::Union(union) => {
                // we accept unions here, but the union types need to be either tables, fields or names
                for ty in union.iter_types() {
                    match ty {
                        Type::Table(_) | Type::Field(_) => {
                            param_variants.push(self.resolve_type_decl_for_param_variant(ty)?)
                        }
                        Type::Name(name) => {
                            let name_str = name.get_type_name().get_name().to_string();
                            let type_decl = self.get_type_decl_for_name(
                                self.0.current_module_path.to_owned(),
                                name_str.to_owned(),
                            )?;

                            let ty =
                                self.resolve_type_decl_for_param_variant(type_decl.get_type())?;
                            param_variants.push(ty);
                        }
                        _ => Err(anyhow::anyhow!("Unsupported union type: {:?}", ty))?,
                    }
                }
            }

            _ => Err(anyhow::anyhow!(
                "Unsupported type declaration: {:?}",
                type_decl.get_type()
            ))?,
        }

        Ok(param_variants)
    }

    /// This function is used when we actually import the type from another file
    /// The way we extract type information is navigating through the main blocks of the ast until we find a
    /// 'export type PARAMS_NAME_WE_NEED' statement
    /// After that we can delegate to one of the functions that we already have
    fn resolve_type_from_extern_file(
        &mut self,
        module_name: String,
        type_name: String,
    ) -> Result<Vec<ParamVariant>, anyhow::Error> {
        match self.0.name_to_module_path.get(&module_name) {
            None => Err(anyhow::anyhow!("Module not found: {}", module_name)),
            Some(module_path) => {
                match self.0.modules.get(module_path) {
									// unreachable for the moment
									Some(ModuleEnum::Resolved(_)) => unreachable!(),
									Some(ModuleEnum::NotYetResolved) => {
											self.0.current_module_path = module_path.to_owned();
											let mut module = Module {
													local_types: HashMap::new(),
													source_code: String::new(),
											};
											// load the module
											let abs_module_path = self
													.0
													.name_to_module_path
													.get(&module_name)
													.ok_or(anyhow::anyhow!("Module not found: {}", module_name))?;

											let module_file = std::fs::read_to_string(abs_module_path).unwrap();
											module.source_code = module_file.clone();
											let module_ast = Parser::default().parse(&module_file).unwrap();

											// first traverse the top-level statements and collect the local types so we can make use of them when needed
											for statement in module_ast.iter_statements() {
													use Statement::*;
													if let TypeDeclaration(type_decl) = statement {
															module.local_types.insert(
																	type_decl.get_name().get_name().to_string(),
																	type_decl.clone(),
															);
													}
											}

											self.0
													.modules
													.insert(module_path.to_owned(), ModuleEnum::Resolved(module));

											let type_decl: &TypeDeclarationStatement = module_ast
													.iter_statements()
													.find_map(|statement| {
															use Statement::*;
															match statement {
																	TypeDeclaration(type_decl) => {
																			if type_decl.get_name().get_name() == &type_name {
																					Some(type_decl)
																			} else {
																					None
																			}
																	}
																	_ => None,
															}
													})
													.ok_or(anyhow::anyhow!("Type declaration not found: {}", type_name))?;

											let res = self.resolve_params_type_decl(type_decl);
											self.0.current_module_path = "".to_owned();
											res
									}
									None => Err(anyhow::anyhow!(
											"Module `{}` found in `name_to_module_path` but not found in `modules` map. Did you forget to add it in code?",
											module_name
									)),
							}
            }
        }
    }

    fn is_user_defined_type(type_name: &str) -> bool {
        !matches!(
            type_name,
            "any"
                | "boolean"
                | "buffer"
                | "never"
                | "nil"
                | "number"
                | "string"
                | "thread"
                | "unknown"
                | "vector"
        )
    }
}

/// Given a relative path, compute its absolute path using the current working directory.
fn absolute_from_cwd<P: AsRef<Path>>(relative: P, cwd: Option<String>) -> std::io::Result<PathBuf> {
    let cwd = cwd.map(PathBuf::from).unwrap_or(env::current_dir()?);
    cwd.join(relative).canonicalize()
}

/// Given a full path and a relative path, compute the resolved full path if the relative path
/// is relative to the full path's parent directory.
fn resolve_relative_to_full<P: AsRef<Path>, R: AsRef<Path>>(
    full: P,
    relative: R,
) -> std::io::Result<PathBuf> {
    let full = full.as_ref();
    let base = full.parent().unwrap_or_else(|| Path::new("/"));
    let joined = base.join(relative);
    joined.canonicalize()
}

impl NodeProcessor for ParamExtractorVisitor {
    fn process_block(&mut self, block: &mut Block) {
        if self.0.main_function.is_some() {
            return;
        }
        for statement in block.iter_statements() {
            use Statement::*;
            match statement {
                // we care about local types = require("./type.luau")
                LocalAssign(local_assign) => {
                    // we look only for local assign statements that have 1 variable and 1 value
                    // we then check to see if the value is a function call to `require`
                    // if it is, we then check to make sure the call has 1 arg and that it is a string
                    // we then try to get that string and resolve the module

                    // oh, by the way, we don't allow types with a depth more than 2 (the type for the Params
                    // can be found only in a required module, not in a require of a required module or so on)
                    if local_assign.variables_len() != 1 || local_assign.values_len() != 1 {
                        continue;
                    }

                    // we know for sure we have 1 variable and 1 value
                    let variable = local_assign.iter_variables().next().unwrap();
                    let value = local_assign.iter_values().next().unwrap();

                    use darklua_core::nodes::Expression;
                    let require_file: Option<String> = match value {
                        Expression::Call(call) => {
                            use darklua_core::nodes::Prefix::*;
                            let prefix_is_okay = match call.get_prefix() {
                                Identifier(identifier) => identifier.get_name() == "require",
                                _ => false,
                            };

                            use darklua_core::nodes::Arguments;

                            let (args_are_okay, arg) = match call.get_arguments() {
                                // Fun fact, Arguments::String is calling the function like so: `require "path"` ?!?!?!?
                                // Arguments::String(string) => (
                                //     true,
                                //     String::from_utf8_lossy(string.get_value()).to_string(),
                                // ),
                                Arguments::Tuple(tuple) => match tuple.len() {
                                    1 => {
                                        let arg = tuple.iter_values().next().unwrap();
                                        match arg {
                                            Expression::String(string) => (
                                                true,
                                                String::from_utf8_lossy(string.get_value())
                                                    .to_string(),
                                            ),
                                            _ => (false, String::new()),
                                        }
                                    }
                                    _ => (false, String::new()),
                                },
                                _ => (false, String::new()),
                            };

                            match (prefix_is_okay, args_are_okay) {
                                (true, true) => Some(arg),
                                _ => None,
                            }
                        }
                        _ => None,
                    };

                    // this is @Lib stuff, we don't care about it atm, SKIP FILE
                    if require_file.is_none() || require_file.as_ref().unwrap().starts_with("@") {
                        continue;
                    }

                    // resolve the full path of the module
                    // this means that we have to make use of the current module path (self.0.main_module_path)
                    // and the require file (require_file)

                    // so, we have to get the FULL path of the main module
                    // THEN, we have to compute the full path of the require file, which we only have relative for the moment
                    let full_path_of_main_module =
                        absolute_from_cwd(self.0.main_module_path.clone(), self.0.cwd.clone())
                            .unwrap();

                    match require_file.clone() {
                        Some(file) => {
                            // append .luau to the file if it doesn't have it
                            let file = if file.ends_with(".luau") {
                                file
                            } else {
                                file + ".luau"
                            };
                            let full_path_of_require_file =
                                resolve_relative_to_full(full_path_of_main_module, file).unwrap();

                            // now, we have to check if the full path of the require file is a file or a directory
                            // if it is a file, we can just use it
                            // if it is a directory, we have to check if it contains a file called "index.luau"
                            // if it does, we can use it, otherwise we have to error out
                            self.0.modules.insert(
                                full_path_of_require_file.to_str().unwrap().to_string(),
                                ModuleEnum::NotYetResolved,
                            );
                            self.0.name_to_module_path.insert(
                                variable.get_name().to_string(),
                                full_path_of_require_file.to_str().unwrap().to_string(),
                            );
                        }
                        None => {}
                    }
                }
                // we care about type Params = ...
                TypeDeclaration(type_decl) => {
                    // delegate the resolution of the type decl so you can also do it for require'd modules
                    // let types: Result<Vec<Type>, _> = self.resolve_type_decl(type_decl);

                    self.0.main_module_types.insert(
                        type_decl.get_name().get_name().to_string(),
                        type_decl.clone(),
                    );
                }
                Function(func) => {
                    if func.get_name().get_name().get_name() == "main" {
                        self.0.main_function = Some(*func.clone());
                    }
                }
                _ => {}
            }
        }

        // after collecting the main function, check if we have a parameter with a type
        let main_function = match self.0.main_function.take() {
            None => {
                self.0.errors.push("No main function found".to_string());
                return;
            }
            Some(func) => func,
        };

        let params = main_function.get_parameters();

        // the flow does not require any parameters, we can just return
        if params.len() == 0 {
            return;
        }

        // the main function should have just 1 parameter which is the Params table
        if params.len() > 1 {
            self.0
                .errors
                .push(format!("Expected 0-1 parameters, got {}", params.len()));
        }

        let param = params.first().unwrap();

        let param_type = match param.get_type() {
            Some(ty) => ty,
            None => {
                self.0.errors.push("No parameter type found".to_string());
                return;
            }
        };

        match param_type {
            Type::Table(table) => match self.resolve_type_table(table).map(|val| vec![val]) {
                Ok(val) => self.0.params = val,
                Err(e) => {
                    self.0
                        .errors
                        .push(format!("Error resolving type table: {e}"));
                    return;
                }
            },
            Type::Field(field) => {
                let module_name = field.get_namespace().get_name().to_string();
                let type_name = field.get_type_name().get_type_name().get_name().to_string();

                // we have to process loaded files, we currently allow only one level of indirection
                match self.resolve_type_from_extern_file(module_name, type_name) {
                    Ok(val) => self.0.params = val,
                    Err(e) => {
                        self.0.errors.push(format!("Error resolving module: {e}"));
                        return;
                    }
                }
            }
            Type::Name(name) => {
                let name_str = name.get_type_name().get_name().to_string();
                if !Self::is_user_defined_type(name_str.as_str()) {
                    self.0.errors.push(format!(
                        "User defined type `{name_str}` is not supported yet"
                    ));
                    return;
                }

                let type_decl = match self.0.main_module_types.get(name_str.as_str()) {
                    Some(type_decl) => type_decl.clone(),
                    None => {
                        self.0
                            .errors
                            .push(format!("Type `{name_str}` not found in main module types"));
                        return;
                    }
                };

                match self.resolve_params_type_decl(&type_decl) {
                    Ok(val) => self.0.params = val,
                    Err(e) => {
                        self.0
                            .errors
                            .push(format!("Error resolving type declaration: {e}"));
                        return;
                    }
                }
            }
            _ => {
                self.0
                    .errors
                    .push(format!("Unsupported parameter type: {param_type:?}"));
                return;
            }
        };
    }
}

pub fn extract_params(
    file: &str,
    file_path: &str,
    cwd: Option<String>,
) -> Result<Vec<ParamVariant>, anyhow::Error> {
    let parser = Parser::default().preserve_tokens();
    let mut ast = parser.parse(file).unwrap();
    let mut visitor = ParamExtractorVisitor::new(cwd);
    visitor.0.main_module_source_code = file.to_string();
    visitor.0.main_module_path = file_path.to_string();
    visitor.process_block(&mut ast);

    if !visitor.0.errors.is_empty() {
        return Err(anyhow::anyhow!("Errors found: {:?}", visitor.0.errors));
    }

    Ok(visitor.0.params)
}
