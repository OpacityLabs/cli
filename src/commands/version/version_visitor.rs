use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use darklua_core::process::{DefaultVisitor, NodeProcessor, NodeVisitor, Scope, ScopeVisitor};
use darklua_core::{nodes, ScopedHashMap};

use crate::commands::version::has_call_to_function_visitor::HasCallToFunctionVisitor;
use crate::commands::version::sdk_version::SdkVersionOut;
use crate::commands::version::utils::get_fqn;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFile {
    /// What default MINIMUM version we should default to
    pub default_version: Option<u64>,
    /// The mappings for each function
    pub function_mappings: HashMap<String, FunctionMapping>,
    /// Function name that lets us know how we figure out the current sdk version
    /// Because sometimes we might have code as such:
    ///
    /// ```
    /// if fetch_sdk_version() > 25 then
    ///    use_function_min_sdk_version_26()
    /// else
    ///    use_function_min_sdk_version_23()
    /// endif
    /// ```
    pub sdk_version_function: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionMapping {
    min_sdk_version: u64,
    #[serde(default)]
    max_sdk_version: Option<u64>,
}

impl From<&FunctionMapping> for SdkVersionOut {
    fn from(function_mapping: &FunctionMapping) -> Self {
        Self {
            min_sdk_version: function_mapping.min_sdk_version,
            max_sdk_version: function_mapping.max_sdk_version,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VersionResolver<'a> {
    variable_scope: ScopedHashMap<String, Option<darklua_core::nodes::Expression>>,
    pub scope_stack: Vec<Box<SdkVersionOut>>,
    scope_data: SdkVersionOut,
    version_file: &'a VersionFile,
}

impl<'a> VersionResolver<'a> {
    pub fn new<'b: 'a>(version_file: &'b VersionFile) -> Self {
        Self {
            scope_stack: Vec::new(),
            scope_data: SdkVersionOut::new(version_file.default_version.unwrap_or(1)),
            version_file,
            variable_scope: ScopedHashMap::default(),
        }
    }

    fn update_scope_data(lhs: &mut SdkVersionOut, rhs: &SdkVersionOut) {
        let merged = SdkVersionOut::sdk_version_intersection(lhs.clone(), rhs.clone());
        *lhs = merged.clone();
    }

    fn update_last_scope_data(&mut self, sdk_version: SdkVersionOut) {
        match self.scope_stack.last_mut() {
            None => {
                self.scope_data.min_sdk_version = sdk_version.min_sdk_version;
                self.scope_data.max_sdk_version = sdk_version.max_sdk_version;
            }
            Some(scope_data) => {
                Self::update_scope_data(scope_data, &sdk_version);
            }
        }
    }

    pub fn sdk_version(&self) -> SdkVersionOut {
        self.scope_data.clone()
    }
}

impl<'a> NodeProcessor for VersionResolver<'a> {
    fn process_function_call(&mut self, call: &mut nodes::FunctionCall) {
        if call.get_method().is_some() {
            return;
        }

        let name = match call.get_prefix() {
            nodes::Prefix::Identifier(identifier) => Some(identifier.get_name().to_string()),
            nodes::Prefix::Field(field) => get_fqn(field),
            _ => None,
        };

        if let Some(name) = name {
            let function_name = if name == "pcall" {
                // if the name is pcall, that means our function should be the first argument to the pcall function
                let args = call.get_arguments().clone();
                if args.len() < 1 {
                    // we don't have any arguments, return, erroneous pcall
                    return;
                }
                match args.to_expressions().first().unwrap() {
                    nodes::Expression::Identifier(identifier) => identifier.get_name().to_string(),
                    nodes::Expression::Field(field) => match get_fqn(&field) {
                        Some(fqn) => fqn,
                        None => return,
                    },
                    _ => return,
                }
            } else {
                name
            };
            if let Some(function_mapping) = self.version_file.function_mappings.get(&function_name)
            {
                self.update_last_scope_data(function_mapping.into());
            }
        }
    }

    fn process_statement(&mut self, statement: &mut nodes::Statement) {
        if let nodes::Statement::If(if_statement) = statement {
            let branches = if_statement.get_branches();

            match branches.len() {
                0 => unreachable!(), // at least 1 branch
                1 => {
                    let else_block = if_statement.get_else_block();

                    match else_block {
                        Some(else_block) => {
                            // if we have both the if and else branch, find the minimum of them 2 and return that
                            // FIRST, find if the conditional of the if branch contains the version_file.sdk_version_function call, otherwise we don't care
                            let mut has_call_to_function_visitor = HasCallToFunctionVisitor::new(
                                self.version_file.sdk_version_function.clone(),
                                &self.variable_scope,
                            );
                            DefaultVisitor::visit_expression(
                                &mut branches[0].get_condition().clone(),
                                &mut has_call_to_function_visitor,
                            );

                            if !has_call_to_function_visitor.has_call_to_function() {
                                return;
                            }

                            let mut cloned_if_block = branches[0].get_block().clone();
                            let mut temp_visitor = VersionResolver::new(self.version_file);
                            ScopeVisitor::visit_block(&mut cloned_if_block, &mut temp_visitor);
                            let if_ver = temp_visitor.sdk_version();

                            let mut cloned_else_block = else_block.clone();
                            let mut temp_visitor = VersionResolver::new(self.version_file);
                            ScopeVisitor::visit_block(&mut cloned_else_block, &mut temp_visitor);
                            let else_ver = temp_visitor.sdk_version();

                            // TODO: should we care about the max version here? usually when we have a check with the sdk_version_function call,
                            // we care solely about the min version (at least for now)
                            let min_sdk_version_full_version =
                                SdkVersionOut::sdk_version_union(if_ver, else_ver);

                            self.update_last_scope_data(min_sdk_version_full_version);

                            clear_if_statement(if_statement);
                        }
                        None => {
                            // if there is just one branch, the if branch, and no else branch, just return the version_file.default_sdk_version
                            let mut has_call_to_function_visitor = HasCallToFunctionVisitor::new(
                                self.version_file.sdk_version_function.clone(),
                                &self.variable_scope,
                            );
                            DefaultVisitor::visit_expression(
                                &mut branches[0].get_condition().clone(),
                                &mut has_call_to_function_visitor,
                            );
                            if !has_call_to_function_visitor.has_call_to_function() {
                                return;
                            }

                            // self.update_last_scope_data(SdkVersionOut::new(
                            //     self.version_file.default_version.unwrap_or(1),
                            // ));

                            clear_if_statement(if_statement);
                        }
                    }
                }
                _ => {
                    // if we have elseifs, unrecognized, TODO, just leave it as it is for now
                }
            }
        }
    }
}

fn clear_if_statement(if_statement: &mut nodes::IfStatement) {
    // we will set every condition to true and every inner block to an empty block
    if_statement
        .mutate_branches()
        .iter_mut()
        .for_each(|branch| {
            *branch.mutate_condition() = nodes::Expression::True(None);
            *branch.mutate_block() = nodes::Block::new(vec![], None);
        });
    if let Some(else_block) = if_statement.mutate_else_block() {
        *else_block = nodes::Block::new(vec![], None);
    }
}

impl<'a> Scope for VersionResolver<'a> {
    fn push(&mut self) {
        self.scope_stack.push(Box::new(SdkVersionOut::new(
            self.version_file.default_version.unwrap_or(1),
        )));
        self.variable_scope.push();
    }
    fn pop(&mut self) {
        if let Some(curr_scope_data) = self.scope_stack.pop() {
            match self.scope_stack.last_mut() {
                None => {
                    // self.scope_data = *curr_scope_data;
                    self.scope_data.min_sdk_version = curr_scope_data.min_sdk_version;
                    self.scope_data.max_sdk_version = curr_scope_data.max_sdk_version;
                }
                Some(prev_scope_data) => {
                    prev_scope_data.min_sdk_version = prev_scope_data
                        .min_sdk_version
                        .max(curr_scope_data.min_sdk_version);
                    let merged = SdkVersionOut::sdk_version_intersection(
                        *prev_scope_data.clone(),
                        *curr_scope_data.clone(),
                    );
                    self.update_last_scope_data(merged);
                }
            }
        }
        self.variable_scope.pop();
    }
    fn insert(&mut self, _identifier: &mut String) {}
    fn insert_local(&mut self, identifier: &mut String, value: Option<&mut nodes::Expression>) {
        self.variable_scope
            .insert(identifier.clone(), value.cloned());
    }
    fn insert_local_function(&mut self, _function: &mut nodes::LocalFunctionStatement) {}
    fn insert_self(&mut self) {}
}

mod test {
    use serde_json::json;

    use super::*;

    #[allow(dead_code)]
    fn get_version_file() -> VersionFile {
        serde_json::from_value(json!({
            "defaultVersion": 10,
            "functionMappings": {
                "get_sdk_version": {
                    "minSdkVersion": 13
                },
                "at_least_20": {
                    "minSdkVersion": 20
                },
                "less_than_20": {
                    "minSdkVersion": 16,
                    "maxSdkVersion": 19
                },
                "global_function_15": {
                    "minSdkVersion": 15
                }
            },
            "sdkVersionFunction": "get_sdk_version"
        }))
        .unwrap()
    }

    #[test]
    fn test_version_visitor_with_if_else_statement() {
        let file = r#"
function main() 
    local x = 33
    for i = 1, 10 do
        local sdk_version = get_sdk_version()
        if sdk_version >= 20 then
            at_least_20()
        else
            less_than_20()
        end
    end
end
        "#;

        let parser = darklua_core::Parser::default();
        let mut block = parser.parse(file).unwrap();

        let version_file = get_version_file();
        let mut version_visitor = VersionResolver::new(&version_file);
        ScopeVisitor::visit_block(&mut block, &mut version_visitor);

        assert!(version_visitor.sdk_version().min_sdk_version == 16)
    }

    #[test]
    fn test_version_visitor_with_if_statement() {
        let file = r#"
function main() 
    local x = 33
    for i = 1, 10 do
        local sdk_version = get_sdk_version()
        if sdk_version >= 20 then
            at_least_20()
        end
    end
end
        "#;

        let parser = darklua_core::Parser::default();
        let mut block = parser.parse(file).unwrap();

        let version_file = get_version_file();
        let mut version_visitor = VersionResolver::new(&version_file);
        ScopeVisitor::visit_block(&mut block, &mut version_visitor);

        // check for the get_sdk_version min version
        assert!(version_visitor.sdk_version().min_sdk_version == 13)
    }

    #[test]
    fn test_version_visitor_with_pcall() {
        let file = r#"
function main() 
    local x = 33
    for i = 1, 10 do
        local my_test_call = pcall(global_function_15)
    end
end
        "#;

        let parser = darklua_core::Parser::default();
        let mut block = parser.parse(file).unwrap();

        let version_file = get_version_file();
        let mut version_visitor = VersionResolver::new(&version_file);
        ScopeVisitor::visit_block(&mut block, &mut version_visitor);

        assert!(version_visitor.sdk_version().min_sdk_version == 15)
    }
}
