//! HNCS primitive conformance tests backed by language-independent JSON vectors.

use std::{error::Error, fmt, fs, path::PathBuf};

use hn_hncs::{
    Decoder, HncsError, write_bool, write_bytes, write_i8, write_i16, write_i32, write_i64,
    write_i128, write_string, write_u8, write_u16, write_u32, write_u64, write_u128,
};
use serde_json::Value;

#[derive(Debug)]
struct VectorError {
    message: String,
}

impl VectorError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for VectorError {}

fn boxed_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(VectorError::new(message))
}

#[test]
fn hncs_primitive_vectors_are_conformant() -> Result<(), Box<dyn Error>> {
    let vectors = load_vectors()?;

    verify_booleans(&vectors)?;
    verify_integers(&vectors)?;
    verify_bytes(&vectors)?;
    verify_strings(&vectors)?;

    Ok(())
}

fn load_vectors() -> Result<Value, Box<dyn Error>> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("..");
    path.push("tests");
    path.push("conformance");
    path.push("core");
    path.push("hncs-primitives-v0.1.json");

    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn verify_booleans(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["booleans", "canonical"])? {
        let value = bool_field(case, "value")?;
        let expected = hex_field(case, "hex")?;

        let mut out = Vec::new();
        write_bool(&mut out, value);
        assert_eq!(out, expected);

        let mut decoder = Decoder::new(&expected);
        assert_eq!(decoder.read_bool()?, value);
        decoder.finish()?;
    }

    for case in array_at(root, &["booleans", "invalid"])? {
        let bytes = hex_field(case, "hex")?;
        let expected = str_field(case, "error")?;
        let mut decoder = Decoder::new(&bytes);

        assert_eq!(error_code(decoder.read_bool()), expected);
    }

    Ok(())
}

fn verify_integers(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["integers", "canonical"])? {
        let integer_type = str_field(case, "type")?;
        let value = str_field(case, "value")?;
        let expected = hex_field(case, "hex")?;

        let encoded = encode_integer(integer_type, value)?;
        assert_eq!(encoded, expected);
        assert_eq!(decode_integer(integer_type, &expected)?, value);
    }

    for case in array_at(root, &["integers", "invalid"])? {
        let integer_type = str_field(case, "type")?;
        let bytes = hex_field(case, "hex")?;
        let expected = str_field(case, "error")?;

        assert_eq!(decode_integer_error(integer_type, &bytes), expected);
    }

    Ok(())
}

fn verify_bytes(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["bytes", "canonical"])? {
        let value = hex_field(case, "value_hex")?;
        let max_len = usize_field(case, "max_len")?;
        let expected = hex_field(case, "hex")?;

        let mut out = Vec::new();
        write_bytes(&mut out, &value, max_len)?;
        assert_eq!(out, expected);

        let mut decoder = Decoder::new(&expected);
        assert_eq!(decoder.read_bytes(max_len)?, value.as_slice());
        decoder.finish()?;
    }

    for case in array_at(root, &["bytes", "invalid"])? {
        let bytes = hex_field(case, "hex")?;
        let max_len = usize_field(case, "max_len")?;
        let expected = str_field(case, "error")?;

        assert_eq!(decode_bytes_error(&bytes, max_len), expected);
    }

    Ok(())
}

fn verify_strings(root: &Value) -> Result<(), Box<dyn Error>> {
    assert_eq!(str_at(root, &["strings", "normalization"])?, "none");

    for case in array_at(root, &["strings", "canonical"])? {
        let value = str_field(case, "value")?;
        let max_len = usize_field(case, "max_len")?;
        let expected = hex_field(case, "hex")?;

        let mut out = Vec::new();
        write_string(&mut out, value, max_len)?;
        assert_eq!(out, expected);

        let mut decoder = Decoder::new(&expected);
        assert_eq!(decoder.read_string(max_len)?, value);
        decoder.finish()?;
    }

    for case in array_at(root, &["strings", "invalid"])? {
        let bytes = hex_field(case, "hex")?;
        let max_len = usize_field(case, "max_len")?;
        let expected = str_field(case, "error")?;

        assert_eq!(decode_string_error(&bytes, max_len), expected);
    }

    Ok(())
}

fn encode_integer(integer_type: &str, value: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();

    match integer_type {
        "u8" => write_u8(&mut out, value.parse()?),
        "u16" => write_u16(&mut out, value.parse()?),
        "u32" => write_u32(&mut out, value.parse()?),
        "u64" => write_u64(&mut out, value.parse()?),
        "u128" => write_u128(&mut out, value.parse()?),
        "i8" => write_i8(&mut out, value.parse()?),
        "i16" => write_i16(&mut out, value.parse()?),
        "i32" => write_i32(&mut out, value.parse()?),
        "i64" => write_i64(&mut out, value.parse()?),
        "i128" => write_i128(&mut out, value.parse()?),
        unsupported => {
            return Err(boxed_error(format!(
                "unsupported integer type {unsupported}"
            )));
        }
    }

    Ok(out)
}

fn decode_integer(integer_type: &str, bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let mut decoder = Decoder::new(bytes);
    let value = match integer_type {
        "u8" => decoder.read_u8()?.to_string(),
        "u16" => decoder.read_u16()?.to_string(),
        "u32" => decoder.read_u32()?.to_string(),
        "u64" => decoder.read_u64()?.to_string(),
        "u128" => decoder.read_u128()?.to_string(),
        "i8" => decoder.read_i8()?.to_string(),
        "i16" => decoder.read_i16()?.to_string(),
        "i32" => decoder.read_i32()?.to_string(),
        "i64" => decoder.read_i64()?.to_string(),
        "i128" => decoder.read_i128()?.to_string(),
        unsupported => {
            return Err(boxed_error(format!(
                "unsupported integer type {unsupported}"
            )));
        }
    };

    decoder.finish()?;
    Ok(value)
}

fn decode_integer_error(integer_type: &str, bytes: &[u8]) -> &'static str {
    let mut decoder = Decoder::new(bytes);
    let result = match integer_type {
        "u8" => decoder.read_u8().map(|_| ()),
        "u16" => decoder.read_u16().map(|_| ()),
        "u32" => decoder.read_u32().map(|_| ()),
        "u64" => decoder.read_u64().map(|_| ()),
        "u128" => decoder.read_u128().map(|_| ()),
        "i8" => decoder.read_i8().map(|_| ()),
        "i16" => decoder.read_i16().map(|_| ()),
        "i32" => decoder.read_i32().map(|_| ()),
        "i64" => decoder.read_i64().map(|_| ()),
        "i128" => decoder.read_i128().map(|_| ()),
        _ => Err(HncsError::InvalidUtf8),
    };

    match result {
        Ok(()) => error_code(decoder.finish()),
        Err(error) => hncs_error_code(&error),
    }
}

fn decode_bytes_error(bytes: &[u8], max_len: usize) -> &'static str {
    let mut decoder = Decoder::new(bytes);
    match decoder.read_bytes(max_len) {
        Ok(_) => error_code(decoder.finish()),
        Err(error) => hncs_error_code(&error),
    }
}

fn decode_string_error(bytes: &[u8], max_len: usize) -> &'static str {
    let mut decoder = Decoder::new(bytes);
    match decoder.read_string(max_len) {
        Ok(_) => error_code(decoder.finish()),
        Err(error) => hncs_error_code(&error),
    }
}

fn error_code<T>(result: Result<T, HncsError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(error) => hncs_error_code(&error),
    }
}

fn hncs_error_code(error: &HncsError) -> &'static str {
    match error {
        HncsError::InvalidBool { .. } => "invalid_bool",
        HncsError::InvalidPresence { .. } => "invalid_presence",
        HncsError::UnexpectedEof { .. } => "unexpected_eof",
        HncsError::LengthLimitExceeded { .. } => "length_limit_exceeded",
        HncsError::CountLimitExceeded { .. } => "count_limit_exceeded",
        HncsError::LengthFieldOverflow { .. } => "length_field_overflow",
        HncsError::InvalidUtf8 => "invalid_utf8",
        HncsError::UnsortedSet => "unsorted_set",
        HncsError::DuplicateSetElement => "duplicate_set_element",
        HncsError::UnsortedMap => "unsorted_map",
        HncsError::DuplicateMapKey => "duplicate_map_key",
        HncsError::TrailingBytes { .. } => "trailing_bytes",
    }
}

fn array_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a [Value], Box<dyn Error>> {
    let value = value_at(value, path)?;
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| boxed_error(format!("path {} is not an array", path_name(path))))
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, Box<dyn Error>> {
    let value = value_at(value, path)?;
    value
        .as_str()
        .ok_or_else(|| boxed_error(format!("path {} is not a string", path_name(path))))
}

fn value_at<'a>(mut value: &'a Value, path: &[&str]) -> Result<&'a Value, Box<dyn Error>> {
    for segment in path {
        value = value
            .get(*segment)
            .ok_or_else(|| boxed_error(format!("missing path {}", path_name(path))))?;
    }

    Ok(value)
}

fn str_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| boxed_error(format!("missing string field {field}")))
}

fn bool_field(value: &Value, field: &str) -> Result<bool, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| boxed_error(format!("missing bool field {field}")))
}

fn usize_field(value: &Value, field: &str) -> Result<usize, Box<dyn Error>> {
    let number = value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| boxed_error(format!("missing usize field {field}")))?;

    Ok(usize::try_from(number)?)
}

fn hex_field(value: &Value, field: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    decode_hex(str_field(value, field)?)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if !value.len().is_multiple_of(2) {
        return Err(boxed_error("hex string has odd length"));
    }

    let mut out = Vec::with_capacity(value.len() / 2);
    for chunk in value.as_bytes().chunks_exact(2) {
        let high = hex_nibble(chunk[0])?;
        let low = hex_nibble(chunk[1])?;
        out.push((high << 4) | low);
    }

    Ok(out)
}

fn hex_nibble(value: u8) -> Result<u8, Box<dyn Error>> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(boxed_error("invalid hex character")),
    }
}

fn path_name(path: &[&str]) -> String {
    path.join(".")
}
