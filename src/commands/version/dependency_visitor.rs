/*
 * Part of this file is derived from the darklua project https://github.com/seaofvoices/darklua
 * which is licensed under the MIT License.
 *
 * Original Copyright (c) 2020 jeparlefrancais
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */
use std::path::{Path, PathBuf};

use bstr::ByteSlice;
use darklua_core::{
    nodes,
    process::NodeProcessor,
    rules::{PathLocator, RequirePathLocator},
};

use crate::commands::version::utils::normalize_path_with_current_dir;

#[derive(Debug)]
pub struct RequireDependencyProcessor<'a, 'b, 'c> {
    depends_on: Vec<PathBuf>,
    current_file_path: PathBuf,
    require_path_locator: RequirePathLocator<'a, 'b, 'c>,
    errors: Vec<anyhow::Error>,
}

const REQUIRE_FUNCTION_IDENTIFIER: &str = "require";

/// This doesn't use an IdentifierTracker like the one from the DarkLua Project
/// As we assume people ONLY use the REQUIRE identifier directly to require files
fn is_require_call(call: &nodes::FunctionCall) -> bool {
    if call.get_method().is_some() {
        return false;
    }

    match call.get_prefix() {
        nodes::Prefix::Identifier(ident) => ident.get_name() == REQUIRE_FUNCTION_IDENTIFIER,
        _ => false,
    }
}

fn convert_string_expression_to_path(string: &nodes::StringExpression) -> Option<&Path> {
    string
        .get_string_value()
        .map(Path::new)
        .or_else(|| bstr::BStr::new(string.get_value()).to_path().ok())
}

pub fn match_path_require_call(call: &nodes::FunctionCall) -> Option<PathBuf> {
    match call.get_arguments() {
        nodes::Arguments::String(string) => convert_string_expression_to_path(string),
        nodes::Arguments::Tuple(tuple) if tuple.len() == 1 => {
            let expression = tuple.iter_values().next().unwrap();

            match expression {
                nodes::Expression::String(string) => convert_string_expression_to_path(string),
                _ => None,
            }
        }
        _ => None,
    }
    .map(normalize_path_with_current_dir)
}

impl<'a, 'b, 'c> RequireDependencyProcessor<'a, 'b, 'c> {
    pub fn new(
        current_file_path: PathBuf,
        require_path_locator: RequirePathLocator<'a, 'b, 'c>,
    ) -> Self {
        Self {
            depends_on: Vec::new(),
            current_file_path,
            require_path_locator,
            errors: Vec::new(),
        }
    }
    fn require_call(&self, call: &nodes::FunctionCall) -> Option<PathBuf> {
        if is_require_call(call) {
            match_path_require_call(call)
        } else {
            None
        }
    }
    fn process(&mut self, call: &nodes::FunctionCall) -> Option<()> {
        let literal_require_path = self.require_call(call)?;

        let require_path = match self
            .require_path_locator
            .find_require_path(literal_require_path, &self.current_file_path)
        {
            Ok(path) => path,
            Err(err) => {
                self.errors
                    .push(anyhow::anyhow!("Failed to find require path: {:?}", err));
                return None;
            }
        };

        self.depends_on.push(require_path);
        Some(())
    }
    pub fn deps(&self) -> &Vec<PathBuf> {
        &self.depends_on
    }
    pub fn errors(&self) -> &Vec<anyhow::Error> {
        &self.errors
    }
}

impl<'a, 'b, 'c> NodeProcessor for RequireDependencyProcessor<'a, 'b, 'c> {
    fn process_expression(&mut self, expression: &mut nodes::Expression) {
        if let nodes::Expression::Call(call) = expression {
            self.process(call);
        }
    }

    fn process_prefix_expression(&mut self, prefix: &mut nodes::Prefix) {
        if let nodes::Prefix::Call(call) = prefix {
            self.process(call);
        }
    }

    fn process_statement(&mut self, statement: &mut nodes::Statement) {
        if let nodes::Statement::Call(call) = statement {
            self.process(call);
        }
    }
}
