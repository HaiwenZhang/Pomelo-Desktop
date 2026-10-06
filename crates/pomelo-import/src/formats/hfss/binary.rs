//! EDB 12.1 typed DEF archive. Unknown schemas and encodings fail at source offsets.
use super::error;
use crate::{ImportContext, ImportError};
use std::collections::{HashMap, HashSet};
pub(super) enum Value {
    Null,
    Number(f64),
    Quantity(f64),
    Text(String),
    Object(Box<Object>),
    Array(Vec<Value>),
}
pub(super) struct Object {
    pub schema: i32,
    pub fields: Vec<Value>,
}
impl Value {
    pub fn object(&self, schema: Option<i32>) -> Result<&Object, ImportError> {
        if let Self::Object(o) = self
            && schema.is_none_or(|s| s == o.schema)
        {
            return Ok(o);
        }
        Err(error("Invalid object schema"))
    }
    pub fn array(&self) -> Result<&[Value], ImportError> {
        if let Self::Array(a) = self {
            Ok(a)
        } else {
            Err(error("Invalid object collection"))
        }
    }
    pub fn integer(&self) -> Result<i32, ImportError> {
        if let Self::Number(n) = self
            && n.fract() == 0.0
            && *n >= i32::MIN as f64
            && *n <= i32::MAX as f64
        {
            return Ok(*n as i32);
        }
        Err(error("Invalid integer reference"))
    }
    pub fn numeric(&self) -> Result<f64, ImportError> {
        if let Self::Number(n) | Self::Quantity(n) = self
            && n.is_finite()
        {
            return Ok(*n);
        }
        Err(error("Invalid numeric value"))
    }
    pub fn quantity(&self) -> Result<f64, ImportError> {
        if let Self::Quantity(n) = self
            && n.is_finite()
        {
            return Ok(*n);
        }
        Err(error("Invalid quantity value"))
    }
    pub fn text(&self) -> Result<&str, ImportError> {
        if let Self::Text(s) = self {
            Ok(s)
        } else {
            Err(error("Invalid text value"))
        }
    }
}
impl Object {
    pub fn field(&self, index: usize) -> Result<&Value, ImportError> {
        self.fields
            .get(index)
            .ok_or_else(|| error(format!("Schema {} missing field {index}", self.schema)))
    }
}
struct Reader<'a, 'b> {
    bytes: &'a [u8],
    at: usize,
    schemas: HashMap<i32, Vec<(usize, u32)>>,
    values: usize,
    context: &'b ImportContext<'b>,
}
impl Reader<'_, '_> {
    fn take(&mut self, len: usize) -> Result<&[u8], ImportError> {
        let end = self
            .at
            .checked_add(len)
            .ok_or_else(|| error("Field length overflow"))?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| error(format!("Truncated DEF @{}", self.at)))?;
        self.at = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, ImportError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, ImportError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, ImportError> {
        Ok(self.u32()? as i32)
    }
    fn f64(&mut self) -> Result<f64, ImportError> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes(b.try_into().map_err(|_| error("f64"))?))
    }
    fn string(&mut self) -> Result<String, ImportError> {
        let len = self.u32()? as usize;
        std::str::from_utf8(self.take(len)?)
            .map(str::to_owned)
            .map_err(|_| error("Invalid DEF UTF-8"))
    }
    fn value(&mut self, kind: u32, depth: usize) -> Result<Value, ImportError> {
        if depth > 100 {
            return Err(error("Record depth exceeds 100"));
        }
        self.values += 1;
        if self.values.is_multiple_of(2048) {
            self.context.check_cancelled()?;
        }
        if self.values > 32_000_000 {
            return Err(ImportError::ResourceLimit {
                actual: self.values as u64,
                limit: 32_000_000,
            });
        }
        Ok(match kind {
            0 => Value::Number(self.u8()? as f64),
            1 => Value::Number(self.i32()? as f64),
            2 => Value::Number(self.f64()?),
            3 => {
                let n = self.f64()?;
                let _expression = self.string()?;
                Value::Quantity(n)
            }
            4 => Value::Text(self.string()?),
            5 => {
                let schema = self.i32()?;
                if schema == -1 {
                    return Ok(Value::Null);
                }
                let schema_fields = self
                    .schemas
                    .get(&schema)
                    .cloned()
                    .ok_or_else(|| error(format!("Undefined schema {schema}")))?;
                let len = schema_fields.iter().map(|f| f.0 + 1).max().unwrap_or(0);
                let mut fields = (0..len).map(|_| Value::Null).collect::<Vec<_>>();
                for (id, kind) in schema_fields {
                    fields[id] = self.value(kind, depth + 1)?;
                }
                Value::Object(Box::new(Object { schema, fields }))
            }
            6 => {
                let count = self.u32()? as usize;
                let kind = self.u32()?;
                if count > 10_000_000 || count > self.bytes.len() - self.at || count > 0 && kind > 8
                {
                    return Err(error("Invalid DEF array"));
                }
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.value(kind, depth + 1)?);
                }
                Value::Array(values)
            }
            7 => {
                let count = self.u32()? as usize;
                if count > 10_000_000 || count > (self.bytes.len() - self.at) / 5 {
                    return Err(error("Invalid DEF property collection"));
                }
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    let kind = self.u32()?;
                    values.push(self.value(kind, depth + 1)?);
                }
                Value::Array(values)
            }
            _ => return Err(error(format!("Unverified DEF field type {kind}"))),
        })
    }
}
pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<Object, ImportError> {
    let mut r = Reader {
        bytes,
        at: 0,
        schemas: HashMap::new(),
        values: 0,
        context,
    };
    if r.u8()? != 0 {
        return Err(error("Invalid DEF header"));
    }
    let header = r.string()?;
    if !header.starts_with("$begin 'Hdr'")
        || !header.lines().any(|l| l.trim() == "Version='12.1'")
        || !header.lines().any(|l| l.trim() == "Encrypted=false")
    {
        return Err(error("Only unencrypted EDB 12.1 DEF is supported"));
    }
    if r.i32()? != -1 {
        return Err(error("Invalid schema marker"));
    }
    let count = r.u32()?;
    if count > 4096 {
        return Err(error("Too many schemas"));
    }
    for _ in 0..count {
        context.check_cancelled()?;
        let id = r.i32()?;
        let count = r.u32()?;
        if count > 256 || r.schemas.contains_key(&id) {
            return Err(error("Duplicate/excessive schema"));
        }
        let mut fields = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..count {
            let id = r.u32()? as usize;
            let kind = r.u32()?;
            if id > 255 || kind > 8 || !seen.insert(id) {
                return Err(error("Invalid schema field"));
            }
            fields.push((id, kind));
        }
        r.schemas.insert(id, fields);
    }
    if r.i32()? != -1 || r.u32()? != 1 {
        return Err(error("Invalid root marker"));
    }
    let root = r.value(5, 0)?;
    if r.i32()? != -1 || r.at != bytes.len() {
        return Err(error("Invalid DEF end marker"));
    }
    let Value::Object(root) = root else {
        return Err(error("Missing DEF root"));
    };
    if root.schema != 0 {
        return Err(error("Invalid DEF root schema"));
    }
    Ok(*root)
}
