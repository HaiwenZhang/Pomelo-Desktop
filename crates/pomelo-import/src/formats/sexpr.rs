//! Bounded S-expression reader for KiCad. Nodes own atoms and retain source structure.
use crate::{ImportContext, ImportError};

/// Board-level expressions are indexed as byte spans, then decoded one object
/// at a time. Large boards must not retain an owning AST for every XY/UUID.
pub(super) struct Document<'a, 'c> {
    text: &'a str,
    spans: Vec<[u32; 2]>,
    context: &'c ImportContext<'c>,
}
impl Document<'_, '_> {
    fn slice(&self, span: [u32; 2]) -> &[u8] {
        &self.text.as_bytes()[span[0] as usize..span[1] as usize]
    }
    fn kind(&self, span: [u32; 2]) -> &str {
        let text = &self.text[span[0] as usize + 1..span[1] as usize];
        let text = text.trim_ascii_start();
        let end = text
            .find(|c: char| c.is_ascii_whitespace() || c == '(' || c == ')')
            .unwrap_or(text.len());
        &text[..end]
    }
    pub fn count(&self, head: &str) -> usize {
        self.spans
            .iter()
            .filter(|&&span| self.kind(span) == head)
            .count()
    }
    pub fn children<'s>(
        &'s self,
        head: &'s str,
    ) -> impl Iterator<Item = Result<Node, ImportError>> + 's {
        self.spans
            .iter()
            .filter(move |&&span| self.kind(span) == head)
            .map(|&span| read(self.slice(span), self.context))
    }
    pub fn required(&self, head: &str) -> Result<Node, ImportError> {
        let span = self
            .spans
            .iter()
            .find(|&&span| self.kind(span) == head)
            .ok_or_else(|| error(format!("kicad_pcb missing {head}")))?;
        read(self.slice(*span), self.context)
    }
}

pub(super) fn document<'a, 'c>(
    bytes: &'a [u8],
    context: &'c ImportContext<'c>,
) -> Result<Document<'a, 'c>, ImportError> {
    const MAX_EXPRESSIONS: usize = 16_000_000;
    context.check_cancelled()?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|_| error("Invalid UTF-8"))?;
    if bytes.len() > u32::MAX as usize {
        return Err(ImportError::ResourceLimit {
            actual: bytes.len() as u64,
            limit: u32::MAX as u64,
        });
    }
    let mut result = Document {
        text,
        spans: Vec::new(),
        context,
    };
    result
        .spans
        .try_reserve_exact((bytes.len() / 256).min(MAX_EXPRESSIONS))
        .map_err(|_| error("Expression index allocation failed"))?;
    let mut cursor = 0;
    let mut next_cancel = 0;
    let mut depth = 0;
    let mut start = 0;
    let mut root = false;
    let mut closed = false;
    while cursor < bytes.len() {
        if cursor >= next_cancel {
            context.check_cancelled()?;
            next_cancel = cursor + 4096;
        }
        match bytes[cursor] {
            b'(' => {
                if depth == 0 {
                    if root || closed {
                        return Err(error("Multiple root expressions"));
                    }
                    root = true;
                    let mut head_start = cursor + 1;
                    while bytes.get(head_start).is_some_and(u8::is_ascii_whitespace) {
                        head_start += 1;
                    }
                    let head_end = bytes[head_start..]
                        .iter()
                        .position(|b| b.is_ascii_whitespace() || *b == b'(' || *b == b')')
                        .map(|i| head_start + i)
                        .unwrap_or(bytes.len());
                    if &bytes[head_start..head_end] != b"kicad_pcb" {
                        return Err(error("Root is not kicad_pcb"));
                    }
                    cursor = head_end;
                    depth = 1;
                    continue;
                }
                if depth >= 256 {
                    return Err(error("Expression depth exceeds 256"));
                }
                if depth == 1 {
                    start = cursor;
                }
                depth += 1;
                cursor += 1;
            }
            b')' => {
                if depth == 0 {
                    return Err(error("Unexpected closing parenthesis"));
                }
                depth -= 1;
                cursor += 1;
                if depth == 1 {
                    if result.spans.len() >= MAX_EXPRESSIONS {
                        return Err(ImportError::ResourceLimit {
                            actual: result.spans.len() as u64 + 1,
                            limit: MAX_EXPRESSIONS as u64,
                        });
                    }
                    if result.spans.len() == result.spans.capacity() {
                        let extra = result
                            .spans
                            .capacity()
                            .max(1024)
                            .min(MAX_EXPRESSIONS - result.spans.len());
                        result
                            .spans
                            .try_reserve_exact(extra)
                            .map_err(|_| error("Expression index allocation failed"))?;
                    }
                    result.spans.push([start as u32, cursor as u32]);
                } else if depth == 0 {
                    closed = true;
                }
            }
            b'"' => {
                if depth == 0 {
                    return Err(error("Atom outside root"));
                }
                cursor += 1;
                loop {
                    if cursor >= next_cancel {
                        context.check_cancelled()?;
                        next_cancel = cursor + 4096;
                    }
                    match bytes.get(cursor) {
                        Some(b'"') => {
                            cursor += 1;
                            break;
                        }
                        Some(b'\\') => {
                            cursor += 2;
                            if cursor > bytes.len() {
                                return Err(error("Truncated escape"));
                            }
                        }
                        Some(_) => cursor += 1,
                        None => return Err(error("Unterminated string")),
                    }
                }
            }
            b if b.is_ascii_whitespace() => cursor += 1,
            _ => {
                if depth == 0 {
                    return Err(error("Atom outside root"));
                }
                cursor += 1;
            }
        }
    }
    context.check_cancelled()?;
    if !root {
        return Err(error("Missing root"));
    }
    if depth != 0 || !closed {
        return Err(error("Unterminated expression"));
    }
    Ok(result)
}
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
    let mut next_cancel = 0usize;
    while cursor < bytes.len() {
        if cursor >= next_cancel {
            context.check_cancelled()?;
            next_cancel = cursor + 4096;
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
                        if cursor >= next_cancel {
                            context.check_cancelled()?;
                            next_cancel = cursor + 4096;
                        }
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
                                    let character = text[cursor..]
                                        .chars()
                                        .next()
                                        .ok_or_else(|| error("Truncated escape"))?;
                                    value.push(character);
                                    cursor += character.len_utf8();
                                    start = cursor;
                                    continue;
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
                        if cursor >= next_cancel {
                            context.check_cancelled()?;
                            next_cancel = cursor + 4096;
                        }
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

#[cfg(test)]
#[path = "../../tests/unit/sexpr_document.rs"]
mod tests;
