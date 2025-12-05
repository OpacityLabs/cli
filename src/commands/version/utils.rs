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
use std::{
    ffi::OsStr,
    iter::FromIterator,
    path::{Component, Path, PathBuf},
};

#[inline]
fn current_dir() -> &'static OsStr {
    OsStr::new(".")
}

#[inline]
fn parent_dir() -> &'static OsStr {
    OsStr::new("..")
}

fn normalize(path: impl AsRef<Path>, keep_current_dir: bool) -> PathBuf {
    let path = path.as_ref();

    if path == Path::new("") {
        return PathBuf::new();
    }

    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        vec![c.as_os_str()]
    } else {
        Vec::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {
                if keep_current_dir && ret.is_empty() {
                    ret.push(current_dir());
                }
            }
            Component::ParentDir => {
                if let Some(last) = ret.last() {
                    let last = *last;
                    if last == current_dir() {
                        ret.pop();
                        ret.push(parent_dir());
                    } else if last != parent_dir() {
                        ret.pop();
                    } else {
                        ret.push(parent_dir());
                    }
                } else {
                    ret.push(parent_dir());
                }
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }

    if ret.is_empty() {
        ret.push(OsStr::new("."));
    }

    PathBuf::from_iter(ret)
}

pub fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    normalize(path, false)
}

pub fn normalize_path_with_current_dir(path: impl AsRef<Path>) -> PathBuf {
    normalize(path, true)
}

/// Get FULLY QUALIFIED NAME
/// Example:
///
/// ```
/// member.expression.inside.member.expression.call()
/// ```
///
/// This gets us
///
/// ```
/// member.expression.inside.member.expression.call
/// ```
///
/// It does NOT work for a any IndexExpression (a.k.a. computed member access)
///
/// ```
/// computed[member].expression.call()
/// ```
///
/// TODO: make it so that it also looks in the scope and resolves identifiers
pub fn get_fqn(field_expression: &darklua_core::nodes::FieldExpression) -> Option<String> {
    use darklua_core::nodes::*;

    match field_expression.get_prefix() {
        Prefix::Identifier(prefix_ident) => {
            // fqn.push_str(identifier.get_name());
            Some(format!(
                "{}.{}",
                prefix_ident.get_name(),
                field_expression.get_field().get_name()
            ))
        }
        Prefix::Field(field) => match get_fqn(field) {
            Some(fqn) => Some(format!(
                "{}.{}",
                fqn,
                field_expression.get_field().get_name()
            )),
            None => None,
        },
        _ => {
            return None;
        }
    }
}
