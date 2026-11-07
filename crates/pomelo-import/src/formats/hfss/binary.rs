//! Self-describing EDB typed DEF archives; compatibility follows fields, not release numbers.
use super::error;
use crate::{ImportContext, ImportError};
use std::collections::{HashMap, HashSet};
#[derive(PartialEq)]
pub(super) enum Value {
    Null,
    Number(f64),
    Quantity(f64),
    Text(String),
    Object(Box<Object>),
    Array(Vec<Value>),
}
#[derive(PartialEq)]
pub(super) struct Object {
    pub schema: i32,
    pub fields: Vec<Value>,
}
pub(super) struct Archive {
    pub root: Object,
    pub version: String,
}

// Across observed 7.3, 8.6, 10.6 and 12.1 archives, these consumed field IDs and
// encodings are stable. Later fields are decoded using the archive's own table.
fn required_fields(schema: i32) -> &'static [(usize, u32)] {
    match schema {
        0 => &[(0, 4), (2, 5)],
        1 => &[(0, 6)],
        2 => &[(1, 6)],
        3 => &[(0, 4), (2, 4), (3, 6), (4, 5)],
        4 => &[(1, 6), (4, 6), (5, 6), (6, 6)],
        5 => &[(0, 1)],
        6 => &[(0, 1), (1, 4)],
        7 => &[(0, 5), (1, 4)],
        10 => &[(0, 5), (1, 1), (2, 1)],
        11 => &[(0, 5), (1, 1), (4, 1)],
        12 => &[
            (0, 5),
            (1, 1),
            (2, 3),
            (3, 3),
            (4, 3),
            (5, 3),
            (6, 3),
            (7, 3),
        ],
        13 => &[(0, 5), (1, 3), (2, 3), (3, 3)],
        14 => &[(0, 5), (1, 1), (2, 1), (3, 1), (4, 3), (6, 5)],
        15 => &[(0, 5), (1, 5)],
        16 => &[(0, 5), (1, 4), (3, 4)],
        19 => &[
            (0, 5),
            (1, 1),
            (2, 3),
            (3, 3),
            (4, 3),
            (6, 4),
            (8, 4),
            (9, 0),
            (10, 0),
            (11, 6),
            (12, 4),
        ],
        21 => &[(0, 5), (1, 4), (2, 4)],
        22 => &[(0, 5)],
        36 => &[(0, 0), (2, 6)],
        51 => &[(0, 5), (1, 5), (2, 5)],
        52 => &[(0, 8)],
        _ => &[],
    }
}

fn validate_schema(schema: i32, fields: &[(usize, u32)]) -> Result<(), ImportError> {
    for &(field, expected) in required_fields(schema) {
        let actual = fields.iter().find(|f| f.0 == field).map(|f| f.1);
        if actual != Some(expected) {
            return Err(error(format!(
                "Incompatible DEF schema {schema} field {field}: expected encoding {expected}, got {actual:?}"
            )));
        }
    }
    Ok(())
}

fn header_version(header: &str) -> Result<String, ImportError> {
    let header = super::text::read(header)?;
    let unencrypted = matches!(
        (
            header.properties.get("Encrypted"),
            header.properties.get("EncryptionLevel"),
        ),
        (Some(super::text::Value::Bool(false)), None)
            | (None, Some(super::text::Value::Number(0.0)))
            | (
                Some(super::text::Value::Bool(false)),
                Some(super::text::Value::Number(0.0))
            )
    );
    if header.name != "Hdr" || !unencrypted {
        return Err(error(
            "Only unencrypted EDB typed DEF archives are supported",
        ));
    }
    let version = header.prop("Version")?.text()?;
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 2
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        || parts[0].parse::<u32>().ok().is_none_or(|major| major == 0)
        || parts[1].parse::<u32>().is_err()
    {
        return Err(error("Invalid EDB archive version"));
    }
    Ok(version.to_owned())
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
    validated_schemas: HashSet<i32>,
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
                if self.validated_schemas.insert(schema) {
                    validate_schema(schema, &schema_fields)
                        .map_err(|e| error(format!("{e} @{}", self.at - 4)))?;
                }
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
            8 => {
                // Packed doubles use a 64-bit count, without per-element type tags.
                let b = self.take(8)?;
                let count = u64::from_le_bytes(b.try_into().map_err(|_| error("array length"))?);
                if count > 10_000_000 || count > ((self.bytes.len() - self.at) / 8) as u64 {
                    return Err(error("Invalid packed DEF array"));
                }
                let mut values = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    values.push(self.value(2, depth + 1)?);
                }
                Value::Array(values)
            }
            _ => return Err(error(format!("Unverified DEF field type {kind}"))),
        })
    }
}
pub(super) fn read(bytes: &[u8], context: &ImportContext<'_>) -> Result<Archive, ImportError> {
    let mut r = Reader {
        bytes,
        at: 0,
        schemas: HashMap::new(),
        validated_schemas: HashSet::new(),
        values: 0,
        context,
    };
    if r.u8()? != 0 {
        return Err(error("Invalid DEF header"));
    }
    let header = r.string()?;
    let version = header_version(&header)?;
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
            if id > 255 || !seen.insert(id) {
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
    Ok(Archive {
        root: *root,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::task::CancellationToken;

    fn archive(version: &str, encrypted: bool) -> Vec<u8> {
        let header =
            format!("$begin 'Hdr'\n\tVersion='{version}'\n\tEncrypted={encrypted}\n$end 'Hdr'\n");
        let mut bytes = vec![0];
        bytes.extend_from_slice(&(header.len() as u32).to_le_bytes());
        bytes.extend_from_slice(header.as_bytes());
        // Root fields are described by IDs, including a nullable child object.
        for value in [-1i32, 1, 0, 2, 0, 4, 2, 5, -1, 1, 0, 0, -1, -1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn imports_compatible_edb_archive_versions() {
        let cancel = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        };
        for version in ["7.3", "8.6", "9.0", "9.5", "10.6", "12.1", "99.0"] {
            let archive = read(&archive(version, false), &context)
                .unwrap_or_else(|error| panic!("EDB {version} must decode: {error}"));
            assert_eq!(archive.version, version);
            assert_eq!(archive.root.schema, 0);
        }
    }

    #[test]
    fn rejects_encrypted_and_truncated_archives() {
        let cancel = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancel,
            progress: &|_| {},
        };
        for version in ["7.3", "8.6", "10.6", "12.1"] {
            assert!(read(&archive(version, true), &context).is_err());
            let mut bytes = archive(version, false);
            bytes.pop();
            assert!(read(&bytes, &context).is_err());
        }
    }

    #[test]
    fn both_encryption_header_conventions_require_explicit_unencrypted_values() {
        for properties in [
            "Encrypted=false",
            "EncryptionLevel=0",
            "Encrypted=false\nEncryptionLevel=0",
        ] {
            let text = format!("$begin 'Hdr'\nVersion='7.6'\n{properties}\n$end 'Hdr'");
            assert_eq!(header_version(&text).unwrap(), "7.6");
        }
        for properties in [
            "",
            "Encrypted=true",
            "EncryptionLevel=1",
            "EncryptionLevel=-1",
            "EncryptionLevel='0'",
            "Encrypted=false\nEncryptionLevel=1",
            "Encrypted=true\nEncryptionLevel=0",
        ] {
            let text = format!("$begin 'Hdr'\nVersion='7.6'\n{properties}\n$end 'Hdr'");
            assert!(header_version(&text).is_err(), "{properties}");
        }
    }

    #[test]
    fn packed_doubles_preserve_coordinates_and_reject_bad_lengths() {
        let context = ImportContext {
            cancellation: &CancellationToken::default(),
            progress: &|_| {},
        };
        let mut bytes = 2u64.to_le_bytes().to_vec();
        bytes.extend(0.25f64.to_le_bytes());
        bytes.extend((-0.5f64).to_le_bytes());
        let huge = u64::MAX.to_le_bytes();
        let excessive = 10_000_001u64.to_le_bytes();
        let decode = |bytes| {
            Reader {
                bytes,
                at: 0,
                schemas: HashMap::new(),
                validated_schemas: HashSet::new(),
                values: 0,
                context: &context,
            }
            .value(8, 0)
        };
        let values = decode(&bytes).unwrap();
        let values = values.array().unwrap();
        assert_eq!(
            [values[0].numeric().unwrap(), values[1].numeric().unwrap()],
            [0.25, -0.5]
        );
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(decode(&huge).is_err());
        assert!(decode(&excessive).is_err());
    }
}
