//! AEDT property grammar parser. Source expressions are parsed as data, never evaluated.
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counted_layer_mapping_and_nested_calls_parse_with_quoted_names() {
        let block = read("$begin 'lm'\nforward[3:0,-1,2]\n$end 'lm'\n").unwrap();
        assert_eq!(block.calls[0].args.len(), 3);
        assert_eq!(block.calls[0].args[1].1.number().unwrap(), -1.0);
        let v = statement("Layer(ID='1', N='Top Copper', T='signal')")
            .unwrap()
            .1;
        assert_eq!(
            v.call().unwrap().arg("N").unwrap().text().unwrap(),
            "Top Copper"
        );
    }
    #[test]
    fn malformed_scope_and_counted_properties_are_rejected() {
        for text in [
            "$begin 'a'\n$end 'b'",
            "forward[2:0]",
            "x=1\nx=2",
            "f(x='unterminated)",
        ] {
            assert!(read(text).is_err(), "{text}");
        }
    }
}
use super::error;
use crate::ImportError;
use std::collections::HashMap;
#[derive(Debug, Clone)]
pub(super) enum Value {
    Text(String),
    Number(f64),
    Bool(bool),
    Call(Call),
}
#[derive(Debug, Clone)]
pub(super) struct Call {
    pub name: String,
    pub args: Vec<(Option<String>, Value)>,
}
#[derive(Default, Debug, Clone)]
pub(super) struct Block {
    pub name: String,
    pub properties: HashMap<String, Value>,
    pub calls: Vec<Call>,
    pub children: Vec<Block>,
}
impl Value {
    pub fn text(&self) -> Result<&str, ImportError> {
        match self {
            Self::Text(s) => Ok(s),
            _ => Err(error("Expected textual property")),
        }
    }
    pub fn number(&self) -> Result<f64, ImportError> {
        match self {
            Self::Number(n) => Ok(*n),
            _ => Err(error(format!("Expected numeric property, got {self:?}"))),
        }
    }
    pub fn call(&self) -> Result<&Call, ImportError> {
        match self {
            Self::Call(c) => Ok(c),
            _ => Err(error("Expected property call")),
        }
    }
}
impl Call {
    pub fn arg(&self, key: &str) -> Result<&Value, ImportError> {
        self.args
            .iter()
            .find(|a| a.0.as_deref() == Some(key))
            .map(|a| &a.1)
            .ok_or_else(|| error(format!("{} missing argument {key}", self.name)))
    }
}
impl Block {
    pub fn require_child(&self, name: &str) -> Result<&Block, ImportError> {
        self.children
            .iter()
            .find(|b| b.name == name)
            .ok_or_else(|| error(format!("Missing property block {name}")))
    }
    pub fn prop(&self, key: &str) -> Result<&Value, ImportError> {
        self.properties
            .get(key)
            .ok_or_else(|| error(format!("Missing property {key}")))
    }
}
struct Parser<'a> {
    text: &'a str,
    at: usize,
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .text
            .as_bytes()
            .get(self.at)
            .is_some_and(|b| b.is_ascii_whitespace())
        {
            self.at += 1;
        }
    }
    fn byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }
    fn token(&mut self) -> Result<(String, bool), ImportError> {
        self.whitespace();
        if self.byte() == Some(b'\'') {
            self.at += 1;
            let mut value = String::new();
            let mut start = self.at;
            loop {
                match self.byte() {
                    Some(b'\'') => {
                        value.push_str(&self.text[start..self.at]);
                        self.at += 1;
                        return Ok((value, true));
                    }
                    Some(b'\\') => {
                        value.push_str(&self.text[start..self.at]);
                        self.at += 1;
                        let b = self
                            .byte()
                            .ok_or_else(|| error("Truncated property escape"))?;
                        value.push(match b {
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b => b as char,
                        });
                        self.at += 1;
                        start = self.at;
                    }
                    Some(_) => self.at += 1,
                    None => return Err(error("Unterminated property string")),
                }
            }
        }
        let start = self.at;
        while self
            .byte()
            .is_some_and(|b| !b.is_ascii_whitespace() && !b"=(),".contains(&b))
        {
            self.at += 1;
        }
        if start == self.at {
            return Err(error("Missing property token"));
        }
        Ok((self.text[start..self.at].into(), false))
    }
    fn value(&mut self, token: (String, bool), depth: usize) -> Result<Value, ImportError> {
        if depth > 64 {
            return Err(error("Property depth exceeds 64"));
        }
        self.whitespace();
        if !token.1 && self.byte() == Some(b'(') {
            self.at += 1;
            let mut args = Vec::new();
            self.whitespace();
            while self.byte() != Some(b')') {
                let token = self.token()?;
                self.whitespace();
                let (key, value) = if self.byte() == Some(b'=') {
                    self.at += 1;
                    let value = self.token()?;
                    (Some(token.0), self.value(value, depth + 1)?)
                } else {
                    (None, self.value(token, depth + 1)?)
                };
                args.push((key, value));
                self.whitespace();
                if self.byte() == Some(b',') {
                    self.at += 1;
                    self.whitespace();
                } else if self.byte() != Some(b')') {
                    return Err(error("Invalid property argument separator"));
                }
            }
            self.at += 1;
            return Ok(Value::Call(Call {
                name: token.0,
                args,
            }));
        }
        if token.1 {
            return Ok(Value::Text(token.0));
        }
        Ok(match token.0.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => {
                if let Ok(n) = token.0.parse::<f64>() {
                    if !n.is_finite() {
                        return Err(error("Non-finite property"));
                    }
                    Value::Number(n)
                } else {
                    Value::Text(token.0)
                }
            }
        })
    }
}
pub(super) fn statement(text: &str) -> Result<(Option<String>, Value), ImportError> {
    let mut p = Parser { text, at: 0 };
    let token = p.token()?;
    p.whitespace();
    let result = if p.byte() == Some(b'=') {
        p.at += 1;
        let value = p.token()?;
        (Some(token.0), p.value(value, 0)?)
    } else {
        (None, p.value(token, 0)?)
    };
    p.whitespace();
    if p.at != text.len() {
        return Err(error("Unexpected property statement suffix"));
    }
    Ok(result)
}
pub(super) fn read(text: &str) -> Result<Block, ImportError> {
    let mut stack = vec![Block::default()];
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut statements = Vec::new();
    for (i, b) in text.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'\'' {
                quoted = false;
            }
        } else if b == b'\'' {
            quoted = true;
        } else if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth = depth
                .checked_sub(1)
                .ok_or_else(|| error("Unbalanced property parentheses"))?;
        } else if b == b'\n' && depth == 0 {
            statements.push(text[start..i].trim());
            start = i + 1;
        }
    }
    if quoted || depth != 0 {
        return Err(error("Truncated property text"));
    }
    if start < text.len() {
        statements.push(text[start..].trim());
    }
    for line in statements.into_iter().filter(|l| !l.is_empty()) {
        if let Some(name) = line.strip_prefix("$begin ") {
            let name = statement(name)?.1.text()?.to_owned();
            if stack.len() > 64 {
                return Err(error("Property block nesting too deep"));
            }
            stack.push(Block {
                name,
                ..Block::default()
            });
        } else if let Some(name) = line.strip_prefix("$end ") {
            let name = statement(name)?.1.text()?.to_owned();
            let child = stack.pop().ok_or_else(|| error("Unexpected block end"))?;
            if child.name != name {
                return Err(error("Property block end mismatch"));
            }
            stack
                .last_mut()
                .ok_or_else(|| error("Unexpected block end"))?
                .children
                .push(child);
        } else {
            let mut rewritten = None;
            let mut count = None;
            if let Some((name, rest)) = line.split_once('[')
                && let Some((n, body)) = rest.split_once(':')
                && let Ok(n) = n.parse::<usize>()
            {
                let body = body
                    .strip_suffix(']')
                    .ok_or_else(|| error("Unterminated counted property"))?;
                rewritten = Some(format!("{name}({body})"));
                count = Some(n);
            }
            let (key, value) = statement(rewritten.as_deref().unwrap_or(line))?;
            if let Some(count) = count
                && value.call()?.args.len() != count
            {
                return Err(error("Counted property length mismatch"));
            }
            let current = stack
                .last_mut()
                .ok_or_else(|| error("Missing property scope"))?;
            if let Some(key) = key {
                if current.properties.insert(key, value).is_some() {
                    return Err(error("Duplicate property"));
                }
            } else if let Value::Call(call) = value {
                current.calls.push(call);
            } else {
                return Err(error("Unknown property line"));
            }
        }
    }
    if stack.len() != 1 {
        return Err(error("Truncated property block"));
    }
    let mut root = stack.pop().ok_or_else(|| error("Missing property root"))?;
    if root.children.len() == 1 && root.calls.is_empty() && root.properties.is_empty() {
        return root
            .children
            .pop()
            .ok_or_else(|| error("Missing property root"));
    }
    Ok(root)
}
