//! Allegro numeric byte order and absolute four-byte string alignment.

use crate::{ImportContext, ImportError, TextEncoding};
use std::cell::RefCell;

/// Only text fields enter the detector; never feed record keys, padding or model payloads.
struct TextSample {
    detector: chardetng::EncodingDetector,
    utf8: bool,
    remaining: usize,
}

impl TextSample {
    fn new() -> Self {
        Self {
            detector: chardetng::EncodingDetector::new(),
            utf8: true,
            remaining: 1024 * 1024,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.utf8 &= std::str::from_utf8(bytes).is_ok();
        if !bytes.is_ascii() && self.remaining > 0 {
            let count = bytes.len().min(self.remaining).min(64 * 1024);
            self.detector.feed(&bytes[..count], false);
            // Text fields are independent strings, rather than pieces of one multibyte character.
            self.detector.feed(b" ", false);
            self.remaining -= count;
        }
    }

    fn encoding(&self) -> TextEncoding {
        if self.utf8 {
            return TextEncoding::Utf8;
        }
        match self.detector.guess(None, false) {
            encoding if encoding == encoding_rs::GBK => TextEncoding::Gbk,
            encoding if encoding == encoding_rs::BIG5 => TextEncoding::Big5,
            encoding if encoding == encoding_rs::SHIFT_JIS => TextEncoding::ShiftJis,
            _ => TextEncoding::Windows1252,
        }
    }
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    encoding: TextEncoding,
    packed_flags: Option<(usize, u16)>,
    sample: Option<RefCell<TextSample>>,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8], encoding: TextEncoding) -> Self {
        Self {
            bytes,
            offset: 0,
            encoding,
            packed_flags: None,
            sample: None,
        }
    }

    pub(super) fn detecting(bytes: &'a [u8]) -> Self {
        let mut reader = Self::new(bytes, TextEncoding::Windows1252);
        reader.sample = Some(RefCell::new(TextSample::new()));
        reader
    }

    pub(super) fn detected_encoding(&self) -> TextEncoding {
        self.sample
            .as_ref()
            .map_or(self.encoding, |sample| sample.borrow().encoding())
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn length(&self) -> usize {
        self.bytes.len()
    }

    pub fn ensure(&self, count: usize) -> Result<(), ImportError> {
        if self
            .offset
            .checked_add(count)
            .is_none_or(|end| end > self.bytes.len())
        {
            return Err(ImportError::OutOfBounds {
                offset: self.offset,
                requested: count,
                length: self.bytes.len(),
            });
        }
        Ok(())
    }

    pub fn seek(&mut self, offset: usize) -> Result<(), ImportError> {
        if offset > self.bytes.len() {
            return Err(ImportError::OutOfBounds {
                offset,
                requested: 0,
                length: self.bytes.len(),
            });
        }
        self.offset = offset;
        Ok(())
    }

    pub fn skip(&mut self, count: usize) -> Result<(), ImportError> {
        self.ensure(count)?;
        self.offset += count;
        Ok(())
    }

    fn read<const N: usize>(&mut self) -> Result<[u8; N], ImportError> {
        self.ensure(N)?;
        let mut result = [0; N];
        result.copy_from_slice(&self.bytes[self.offset..self.offset + N]);
        self.offset += N;
        Ok(result)
    }

    /// V15 consumes one byte for the tag, then exposes the packed ten-bit flags.
    pub fn record_type(&mut self, version: u16) -> Result<u8, ImportError> {
        self.packed_flags = None;
        if version >= 160 {
            return Ok(self.u8()? as u8);
        }
        self.ensure(2)?;
        let tag = u16::from_le_bytes([self.bytes[self.offset], self.bytes[self.offset + 1]]);
        self.packed_flags = Some((self.offset + 1, tag & 0x3ff));
        self.offset += 1;
        Ok((tag >> 10) as u8)
    }

    /// Logical byte values can exceed 255 for a packed V15 flags byte.
    pub fn u8(&mut self) -> Result<u16, ImportError> {
        let offset = self.offset;
        let raw = self.read::<1>()?[0];
        Ok(self
            .packed_flags
            .filter(|(position, _)| *position == offset)
            .map_or(u16::from(raw), |(_, flags)| flags))
    }

    pub fn u16(&mut self) -> Result<u16, ImportError> {
        Ok(u16::from_le_bytes(self.read()?))
    }
    pub fn i16(&mut self) -> Result<i16, ImportError> {
        Ok(i16::from_le_bytes(self.read()?))
    }
    pub fn u32(&mut self) -> Result<u32, ImportError> {
        Ok(u32::from_le_bytes(self.read()?))
    }
    pub fn i32(&mut self) -> Result<i32, ImportError> {
        Ok(i32::from_le_bytes(self.read()?))
    }

    pub(super) fn u16s(&mut self, count: usize) -> Result<Vec<u16>, ImportError> {
        self.fixed_array(count, 2, Self::u16)
    }
    pub(super) fn u32s(&mut self, count: usize) -> Result<Vec<u32>, ImportError> {
        self.fixed_array(count, 4, Self::u32)
    }
    pub(super) fn i32s(&mut self, count: usize) -> Result<Vec<i32>, ImportError> {
        self.fixed_array(count, 4, Self::i32)
    }
    fn fixed_array<T>(
        &mut self,
        count: usize,
        width: usize,
        read: fn(&mut Self) -> Result<T, ImportError>,
    ) -> Result<Vec<T>, ImportError> {
        // All fixed layouts declare at most 100 entries. Variable counts use a separate budget.
        if count > 100 {
            return Err(ImportError::InvalidRecord {
                offset: self.offset,
                field: "FIXED_ARRAY_COUNT",
                value: count as u64,
            });
        }
        self.ensure(count * width)?;
        (0..count).map(|_| read(self)).collect()
    }

    /// Two little-endian words, high word first, form one IEEE-754 double.
    pub fn float(&mut self) -> Result<f64, ImportError> {
        let high = u64::from(self.u32()?);
        let low = u64::from(self.u32()?);
        Ok(f64::from_bits((high << 32) | low))
    }

    pub fn fixed_string(&mut self, length: usize) -> Result<String, ImportError> {
        self.ensure(length)?;
        let start = self.offset;
        let bytes = &self.bytes[start..start + length];
        let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(length);
        let value = self.decode(&bytes[..end], start)?;
        self.skip(length)?;
        self.align()?;
        Ok(value)
    }

    /// Borrow opaque bytes, retaining embedded NULs and absolute four-byte alignment.
    pub fn bytes(&mut self, length: usize) -> Result<&'a [u8], ImportError> {
        self.ensure(length)?;
        let start = self.offset;
        self.skip(length)?;
        self.align()?;
        Ok(&self.bytes[start..start + length])
    }

    pub fn cstring(&mut self) -> Result<String, ImportError> {
        let start = self.offset;
        let end = (start..self.bytes.len())
            .find(|&position| {
                self.packed_flags
                    .filter(|(flag_position, _)| *flag_position == position)
                    .map_or(self.bytes[position] == 0, |(_, flags)| flags == 0)
            })
            .ok_or(ImportError::OutOfBounds {
                offset: self.bytes.len(),
                requested: 1,
                length: self.bytes.len(),
            })?;
        let value = self.decode(&self.bytes[start..end], start)?;
        self.offset = end + 1;
        self.align()?;
        Ok(value)
    }

    /// String-table path: bounded allocation and cancellation during long byte scans.
    pub fn bounded_cstring(
        &mut self,
        max_bytes: usize,
        context: &ImportContext<'_>,
    ) -> Result<String, ImportError> {
        let start = self.offset;
        let available = self.bytes.len() - start;
        let budget = available.min(max_bytes.saturating_add(1));
        for chunk_start in (0..budget).step_by(64 * 1024) {
            context.check_cancelled()?;
            let chunk_end = budget.min(chunk_start + 64 * 1024);
            if let Some(index) = self.bytes[start + chunk_start..start + chunk_end]
                .iter()
                .position(|&byte| byte == 0)
            {
                let end = start + chunk_start + index;
                let value = self.decode(&self.bytes[start..end], start)?;
                self.offset = end + 1;
                self.align()?;
                return Ok(value);
            }
        }
        if available > max_bytes {
            Err(ImportError::IndexLimit {
                actual: max_bytes.saturating_add(1) as u64,
                limit: max_bytes as u64,
            })
        } else {
            Err(ImportError::OutOfBounds {
                offset: self.bytes.len(),
                requested: 1,
                length: self.bytes.len(),
            })
        }
    }

    pub(super) fn align(&mut self) -> Result<(), ImportError> {
        self.skip((4 - self.offset % 4) % 4)
    }

    fn decode(&self, bytes: &[u8], offset: usize) -> Result<String, ImportError> {
        if let Some(sample) = &self.sample {
            sample.borrow_mut().feed(bytes);
        }
        let selected = if matches!(self.encoding, TextEncoding::Auto) {
            // Header-only probes have no board-wide sample. Full imports resolve Auto in BrdIndex.
            let mut sample = TextSample::new();
            sample.feed(bytes);
            sample.encoding()
        } else {
            self.encoding
        };
        // Match TextDecoder's default UTF-8 BOM handling in the frozen Web reader.
        // Keep the original source offset for diagnostics and other code pages unchanged.
        let bytes = if matches!(selected, TextEncoding::Utf8) {
            bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)
        } else {
            bytes
        };
        let encoding = match selected {
            TextEncoding::Auto => encoding_rs::UTF_8,
            TextEncoding::Utf8 => encoding_rs::UTF_8,
            TextEncoding::Gbk => encoding_rs::GBK,
            TextEncoding::ShiftJis => encoding_rs::SHIFT_JIS,
            TextEncoding::Big5 => encoding_rs::BIG5,
            TextEncoding::Windows1252 => encoding_rs::WINDOWS_1252,
        };
        encoding
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|text| text.into_owned())
            .ok_or(ImportError::InvalidEncoding(offset))
    }
}
