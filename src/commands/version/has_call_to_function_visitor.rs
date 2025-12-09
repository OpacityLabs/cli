use darklua_core::process::NodeProcessor;
use darklua_core::{nodes, ScopedHashMap};

use crate::commands::version::utils::get_fqn;

pub struct HasCallToFunctionVisitor<'a> {
    function_name: String,
    has_call_to_function_field: bool,
    variable_scope: &'a ScopedHashMap<String, Option<darklua_core::nodes::Expression>>,
}

impl<'a> HasCallToFunctionVisitor<'a> {
    pub fn new(
        function_name: String,
        variable_scope: &'a ScopedHashMap<String, Option<darklua_core::nodes::Expression>>,
    ) -> Self {
        Self {
            function_name,
            has_call_to_function_field: false,
            variable_scope,
        }
    }

    pub fn has_call_to_function(&self) -> bool {
        self.has_call_to_function_field
    }
}

impl<'a> NodeProcessor for HasCallToFunctionVisitor<'a> {
    fn process_expression(&mut self, expression: &mut nodes::Expression) {
        match expression {
            nodes::Expression::Identifier(binary) => {
                let name = binary.get_name().to_string();
                if let Some(maybe_expr) = self.variable_scope.get(&name) {
                    if let Some(expr) = maybe_expr {
                        if let nodes::Expression::Call(call) = expr {
                            if let nodes::Prefix::Identifier(identifier) = call.get_prefix() {
                                if identifier.get_name().to_string() == self.function_name {
                                    self.has_call_to_function_field = true;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

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
            if name == self.function_name {
                self.has_call_to_function_field = true;
            }
        }
    }
}
