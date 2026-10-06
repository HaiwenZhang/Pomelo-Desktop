//! Bounded S-expression reader for KiCad. Nodes own atoms and retain source structure.
use crate::{ImportContext, ImportError};
#[derive(Debug)]
pub(super) enum Value {
    Atom(String),
    List(Node),
}
#[derive(Debug)]
pub(super) struct Node {
    pub values: Vec<Value>,
}
pub(super) fn error(details: impl Into<String>) -> ImportError {
    ImportError::Format {
        format: "KiCad".into(),
        details: details.into(),
    }
}
impl Node {
    pub fn atom(&self, index: usize) -> Result<&str, ImportError> {
        match self.values.get(index) {
            Some(Value::Atom(a)) => Ok(a),
            _ => Err(error("Missing expression atom")),
        }
    }
    pub fn head(&self) -> &str {
        self.atom(0).unwrap_or("")
    }
    pub fn children<'a>(&'a self, head: &'a str) -> impl Iterator<Item = &'a Node> {
        self.values.iter().filter_map(move |v| match v {
            Value::List(n) if n.head() == head => Some(n),
            _ => None,
        })
    }
    pub fn find_child(&self, head: &str) -> Option<&Node> {
        self.values.iter().find_map(|v| match v {
            Value::List(n) if n.head() == head => Some(n),
            _ => None,
        })
    }
    pub fn required(&self, head: &str) -> Result<&Node, ImportError> {
        self.find_child(head)
            .ok_or_else(|| error(format!("{} missing {head}", self.head())))
    }
    pub fn number(&self, index: usize) -> Result<f64, ImportError> {
        let value = self
            .atom(index)?
            .parse::<f64>()
            .map_err(|_| error("Invalid numeric atom"))?;
        if !value.is_finite() {
            return Err(error("Non-finite numeric atom"));
        }
        Ok(value)
    }
    pub fn number_or(&self, index: usize, default: f64) -> Result<f64, ImportError> {
        if self.values.get(index).is_none() {
            Ok(default)
        } else {
            self.number(index)
        }
    }
}
pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<Node, ImportError> {
    let text = std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
        .map_err(|_| error("Invalid UTF-8"))?;
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut stack = Vec::<Node>::new();
    let mut root = None;
    let mut count = 0usize;
    while cursor < bytes.len() {
        if cursor % 4096 == 0 || count.is_multiple_of(1024) {
            context.check_cancelled()?;
        }
        match bytes[cursor] {
            b' ' | b'\r' | b'\n' | b'\t' => {
                cursor += 1;
            }
            b'(' => {
                if stack.len() >= 256 {
                    return Err(error("Expression depth exceeds 256"));
                }
                stack.push(Node { values: Vec::new() });
                cursor += 1;
            }
            b')' => {
                let node = stack
                    .pop()
                    .ok_or_else(|| error("Unexpected closing parenthesis"))?;
                cursor += 1;
                if let Some(parent) = stack.last_mut() {
                    parent.values.push(Value::List(node));
                } else if root.replace(node).is_some() {
                    return Err(error("Multiple root expressions"));
                }
            }
            _ => {
                let atom = if bytes[cursor] == b'"' {
                    cursor += 1;
                    let mut value = String::new();
                    let mut start = cursor;
                    loop {
                        let byte = *bytes
                            .get(cursor)
                            .ok_or_else(|| error("Unterminated string"))?;
                        if byte == b'"' {
                            value.push_str(&text[start..cursor]);
                            cursor += 1;
                            break;
                        }
                        if byte == b'\\' {
                            value.push_str(&text[start..cursor]);
                            cursor += 1;
                            let escaped =
                                *bytes.get(cursor).ok_or_else(|| error("Truncated escape"))?;
                            match escaped {
                                b'n' => value.push('\n'),
                                b'r' => value.push('\r'),
                                b't' => value.push('\t'),
                                b'"' => value.push('"'),
                                b'\\' => value.push('\\'),
                                _ => {
                                    value.push('\\');
                                    value.push(escaped as char);
                                }
                            }
                            cursor += 1;
                            start = cursor;
                        } else {
                            cursor += 1;
                        }
                    }
                    value
                } else {
                    let start = cursor;
                    while cursor < bytes.len()
                        && !bytes[cursor].is_ascii_whitespace()
                        && bytes[cursor] != b'('
                        && bytes[cursor] != b')'
                    {
                        cursor += 1;
                    }
                    text[start..cursor].to_owned()
                };
                stack
                    .last_mut()
                    .ok_or_else(|| error("Atom outside root"))?
                    .values
                    .push(Value::Atom(atom));
                count += 1;
                if count > 16_000_000 {
                    return Err(ImportError::ResourceLimit {
                        actual: count as u64,
                        limit: 16_000_000,
                    });
                }
            }
        }
    }
    if !stack.is_empty() {
        return Err(error("Unterminated expression"));
    }
    root.ok_or_else(|| error("Missing root"))
}
