//! Bytes <-> text for `.properties` files, with encoding detection.
//!
//! Invariant: for every byte string `b`, `encode(&d.text, d.encoding) == b` where
//! `d = decode(b, None).unwrap()`. Decoding never fails without a hint, and saving an
//! unchanged file writes the original bytes back.
//! Pure functions on bytes: no file access, no line handling, no `\uXXXX` parsing.

pub use types::Encoding;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub encoding: Encoding,
    pub text: String,
}

/// Only possible when a hint forces an encoding that cannot represent the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    InvalidUtf8,
    /// Windows-1252 leaves 0x81, 0x8D, 0x8F, 0x90 and 0x9D undefined.
    UndefinedByte(u8),
}

const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

/// Windows-1252 for the bytes 0x80..=0x9F, the only range where it differs from Latin-1.
/// 0 marks the five undefined bytes.
const CP1252_C1: [u16; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, //
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0, //
    0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, //
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

fn is_c1(byte: u8) -> bool {
    (0x80..=0x9F).contains(&byte)
}

fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Returns the first undefined byte as the error.
fn decode_windows1252(bytes: &[u8]) -> Result<String, u8> {
    bytes
        .iter()
        .map(|&b| {
            if !is_c1(b) {
                return Ok(char::from(b));
            }
            match CP1252_C1[usize::from(b - 0x80)] {
                0 => Err(b),
                unit => Ok(char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER)),
            }
        })
        .collect()
}

fn decode_utf8(bytes: &[u8]) -> Result<String, DecodeError> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| DecodeError::InvalidUtf8)
}

/// Without a hint the encoding is detected: UTF-8 BOM, then valid UTF-8, then Windows-1252
/// if a byte in 0x80..=0x9F occurs (and all bytes are defined there), else Latin-1.
pub fn decode(bytes: &[u8], hint: Option<Encoding>) -> Result<Decoded, DecodeError> {
    match hint {
        Some(Encoding::Utf8) => Ok(Decoded {
            encoding: Encoding::Utf8,
            text: decode_utf8(bytes)?,
        }),
        Some(Encoding::Utf8Bom) => Ok(Decoded {
            encoding: Encoding::Utf8Bom,
            text: decode_utf8(bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes))?,
        }),
        Some(Encoding::Latin1) => Ok(Decoded {
            encoding: Encoding::Latin1,
            text: decode_latin1(bytes),
        }),
        Some(Encoding::Windows1252) => Ok(Decoded {
            encoding: Encoding::Windows1252,
            text: decode_windows1252(bytes).map_err(DecodeError::UndefinedByte)?,
        }),
        None => Ok(detect(bytes)),
    }
}

fn detect(bytes: &[u8]) -> Decoded {
    if let Some(text) = bytes
        .strip_prefix(UTF8_BOM)
        .and_then(|rest| std::str::from_utf8(rest).ok())
    {
        return Decoded {
            encoding: Encoding::Utf8Bom,
            text: text.to_owned(),
        };
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Decoded {
            encoding: Encoding::Utf8,
            text: text.to_owned(),
        };
    }
    if bytes.iter().copied().any(is_c1)
        && let Ok(text) = decode_windows1252(bytes)
    {
        return Decoded {
            encoding: Encoding::Windows1252,
            text,
        };
    }
    Decoded {
        encoding: Encoding::Latin1,
        text: decode_latin1(bytes),
    }
}

fn encode_windows1252_char(c: char) -> Option<u8> {
    match u32::from(c) {
        cp @ (0..=0x7F | 0xA0..=0xFF) => u8::try_from(cp).ok(),
        cp => {
            let index = CP1252_C1
                .iter()
                .position(|&unit| unit != 0 && u32::from(unit) == cp)?;
            u8::try_from(0x80 + index).ok()
        }
    }
}

fn push_escaped(out: &mut Vec<u8>, c: char) {
    let mut units = [0u16; 2];
    for unit in c.encode_utf16(&mut units) {
        out.extend_from_slice(format!("\\u{unit:04X}").as_bytes());
    }
}

/// Characters the encoding cannot represent are written as Java-style `\uXXXX` escapes
/// (uppercase hex, surrogate pairs outside the BMP), so saving never fails.
pub fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    let single_byte: fn(char) -> Option<u8> = match encoding {
        Encoding::Utf8 => return text.as_bytes().to_vec(),
        Encoding::Utf8Bom => {
            let mut out = UTF8_BOM.to_vec();
            out.extend_from_slice(text.as_bytes());
            return out;
        }
        Encoding::Latin1 => |c| u8::try_from(u32::from(c)).ok(),
        Encoding::Windows1252 => encode_windows1252_char,
    };
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        match single_byte(c) {
            Some(byte) => out.push(byte),
            None => push_escaped(&mut out, c),
        }
    }
    out
}
