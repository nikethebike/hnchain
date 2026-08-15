//! HNCS compound conformance tests backed by language-independent JSON vectors.

use std::{error::Error, fmt, fs, path::PathBuf};

use hn_hncs::{
    Decoder, HncsError, HncsResult, write_bytes, write_list, write_map, write_optional, write_set,
    write_string, write_u8,
};
use serde_json::Value;

type BytesU8Entries = Vec<(Vec<u8>, u8)>;

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
fn hncs_compound_vectors_are_conformant() -> Result<(), Box<dyn Error>> {
    let vectors = load_vectors()?;

    verify_optional(&vectors)?;
    verify_lists(&vectors)?;
    verify_sets(&vectors)?;
    verify_maps(&vectors)?;

    Ok(())
}

fn load_vectors() -> Result<Value, Box<dyn Error>> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("..");
    path.push("tests");
    path.push("conformance");
    path.push("core");
    path.push("hncs-compound-v0.1.json");

    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn verify_optional(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["optional", "canonical"])? {
        let value_type = str_field(case, "type")?;
        let expected = hex_field(case, "hex")?;
        let encoded = encode_optional_case(case, value_type)?;

        assert_eq!(encoded, expected);
        assert_eq!(decode_optional_error(value_type, case, &expected), "ok");
    }

    for case in array_at(root, &["optional", "invalid"])? {
        let value_type = str_field(case, "type")?;
        let bytes = hex_field(case, "hex")?;
        let expected = str_field(case, "error")?;

        assert_eq!(decode_optional_error(value_type, case, &bytes), expected);
    }

    Ok(())
}

fn verify_lists(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["lists", "canonical"])? {
        let value_type = str_field(case, "type")?;
        let expected = hex_field(case, "hex")?;
        let encoded = encode_list_case(case, value_type)?;

        assert_eq!(encoded, expected);
        assert_eq!(decode_list_error(value_type, case, &expected), "ok");
    }

    for case in array_at(root, &["lists", "inequality"])? {
        let left = hex_field(case, "left_hex")?;
        let right = hex_field(case, "right_hex")?;
        let expected_equal = bool_field(case, "expected_equal")?;

        assert_eq!(left == right, expected_equal);
    }

    for case in array_at(root, &["lists", "invalid"])? {
        let value_type = str_field(case, "type")?;
        let bytes = hex_field(case, "hex")?;
        let expected = str_field(case, "error")?;

        assert_eq!(decode_list_error(value_type, case, &bytes), expected);
    }

    Ok(())
}

fn verify_sets(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["sets", "canonical"])? {
        let value_type = str_field(case, "type")?;
        let expected = hex_field(case, "hex")?;

        for semantic_input in array_field(case, "semantic_inputs")? {
            let encoded = encode_set_case(case, value_type, semantic_input)?;
            assert_eq!(encoded, expected);
        }

        assert_eq!(decode_set_error(value_type, case, &expected), "ok");
    }

    for case in array_at(root, &["sets", "invalid"])? {
        let value_type = str_field(case, "type")?;
        let bytes = hex_field(case, "hex")?;
        let expected = str_field(case, "error")?;

        assert_eq!(decode_set_error(value_type, case, &bytes), expected);
    }

    Ok(())
}

fn verify_maps(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["maps", "canonical"])? {
        let value_type = str_field(case, "type")?;
        let expected = hex_field(case, "hex")?;

        for semantic_input in array_field(case, "semantic_inputs")? {
            let encoded = encode_map_case(case, value_type, semantic_input)?;
            assert_eq!(encoded, expected);
        }

        assert_eq!(decode_map_error(value_type, case, &expected), "ok");
    }

    for case in array_at(root, &["maps", "invalid"])? {
        let value_type = str_field(case, "type")?;
        let bytes = hex_field(case, "hex")?;
        let expected = str_field(case, "error")?;

        assert_eq!(decode_map_error(value_type, case, &bytes), expected);
    }

    Ok(())
}

fn encode_optional_case(case: &Value, value_type: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    match value_type {
        "optional<u8>" => {
            let value = optional_string_field(case, "value")?
                .map(str::parse)
                .transpose()?;
            write_optional(&mut out, value.as_ref(), write_u8_value)?;
        }
        "optional<string>" => {
            let max_len = usize_field(case, "max_len")?;
            let value = optional_string_field(case, "value")?;
            write_optional(&mut out, value, |out, value| {
                write_string(out, value, max_len)
            })?;
        }
        unsupported => {
            return Err(boxed_error(format!(
                "unsupported optional type {unsupported}"
            )));
        }
    }

    Ok(out)
}

fn encode_list_case(case: &Value, value_type: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    match value_type {
        "list<u8>" => {
            let values = string_array_field(case, "value")?
                .iter()
                .map(|value| value.parse())
                .collect::<Result<Vec<u8>, _>>()?;
            write_list(
                &mut out,
                &values,
                usize_field(case, "max_count")?,
                write_u8_value,
            )?;
        }
        unsupported => return Err(boxed_error(format!("unsupported list type {unsupported}"))),
    }

    Ok(out)
}

fn encode_set_case(
    case: &Value,
    value_type: &str,
    semantic_input: &Value,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    match value_type {
        "set<u8>" => {
            let values = string_array(semantic_input)?
                .iter()
                .map(|value| value.parse())
                .collect::<Result<Vec<u8>, _>>()?;
            write_set(
                &mut out,
                &values,
                usize_field(case, "max_count")?,
                write_u8_value,
            )?;
        }
        "set<bytes>" => {
            let values = string_array(semantic_input)?
                .iter()
                .map(|value| decode_hex(value))
                .collect::<Result<Vec<Vec<u8>>, _>>()?;
            let element_max_len = usize_field(case, "element_max_len")?;
            write_set(
                &mut out,
                &values,
                usize_field(case, "max_count")?,
                |out, value| write_bytes(out, value, element_max_len),
            )?;
        }
        unsupported => return Err(boxed_error(format!("unsupported set type {unsupported}"))),
    }

    Ok(out)
}

fn encode_map_case(
    case: &Value,
    value_type: &str,
    semantic_input: &Value,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    match value_type {
        "map<u8,u8>" => {
            let entries = parse_u8_u8_entries(semantic_input)?;
            write_map(
                &mut out,
                &entries,
                usize_field(case, "max_count")?,
                write_u8_value,
                write_u8_value,
            )?;
        }
        "map<bytes,u8>" => {
            let entries = parse_bytes_u8_entries(semantic_input)?;
            let key_max_len = usize_field(case, "key_max_len")?;
            write_map(
                &mut out,
                &entries,
                usize_field(case, "max_count")?,
                |out, value| write_bytes(out, value, key_max_len),
                write_u8_value,
            )?;
        }
        unsupported => return Err(boxed_error(format!("unsupported map type {unsupported}"))),
    }

    Ok(out)
}

fn decode_optional_error(value_type: &str, case: &Value, bytes: &[u8]) -> &'static str {
    let mut decoder = Decoder::new(bytes);
    let result = match value_type {
        "optional<u8>" => decoder.read_optional(read_u8_value).map(|_| ()),
        "optional<u16>" => decoder
            .read_optional(|decoder| decoder.read_u16())
            .map(|_| ()),
        "optional<string>" => {
            let max_len = usize_field(case, "max_len").unwrap_or(0);
            decoder
                .read_optional(|decoder| decoder.read_string(max_len))
                .map(|_| ())
        }
        _ => Err(HncsError::InvalidUtf8),
    };

    finish_or_error(result, &decoder)
}

fn decode_list_error(value_type: &str, case: &Value, bytes: &[u8]) -> &'static str {
    let mut decoder = Decoder::new(bytes);
    let max_count = usize_field(case, "max_count").unwrap_or(0);
    let result = match value_type {
        "list<u8>" => decoder.read_list(max_count, read_u8_value).map(|_| ()),
        _ => Err(HncsError::InvalidUtf8),
    };

    finish_or_error(result, &decoder)
}

fn decode_set_error(value_type: &str, case: &Value, bytes: &[u8]) -> &'static str {
    let mut decoder = Decoder::new(bytes);
    let max_count = usize_field(case, "max_count").unwrap_or(0);
    let result = match value_type {
        "set<u8>" => decoder.read_set(max_count, read_u8_value).map(|_| ()),
        "set<bytes>" => {
            let element_max_len = usize_field(case, "element_max_len").unwrap_or(0);
            decoder
                .read_set(max_count, |decoder| decoder.read_bytes(element_max_len))
                .map(|_| ())
        }
        _ => Err(HncsError::InvalidUtf8),
    };

    finish_or_error(result, &decoder)
}

fn decode_map_error(value_type: &str, case: &Value, bytes: &[u8]) -> &'static str {
    let mut decoder = Decoder::new(bytes);
    let max_count = usize_field(case, "max_count").unwrap_or(0);
    let result = match value_type {
        "map<u8,u8>" => decoder
            .read_map(max_count, read_u8_value, read_u8_value)
            .map(|_| ()),
        "map<bytes,u8>" => {
            let key_max_len = usize_field(case, "key_max_len").unwrap_or(0);
            decoder
                .read_map(
                    max_count,
                    |decoder| decoder.read_bytes(key_max_len),
                    read_u8_value,
                )
                .map(|_| ())
        }
        _ => Err(HncsError::InvalidUtf8),
    };

    finish_or_error(result, &decoder)
}

fn finish_or_error(result: HncsResult<()>, decoder: &Decoder<'_>) -> &'static str {
    match result {
        Ok(()) => error_code(decoder.finish()),
        Err(error) => hncs_error_code(&error),
    }
}

fn parse_u8_u8_entries(value: &Value) -> Result<Vec<(u8, u8)>, Box<dyn Error>> {
    array(value)?
        .iter()
        .map(|entry| {
            let pair = string_array(entry)?;
            if pair.len() != 2 {
                return Err(boxed_error("map entry must contain exactly two values"));
            }

            Ok((pair[0].parse()?, pair[1].parse()?))
        })
        .collect()
}

fn parse_bytes_u8_entries(value: &Value) -> Result<BytesU8Entries, Box<dyn Error>> {
    array(value)?
        .iter()
        .map(|entry| {
            let pair = string_array(entry)?;
            if pair.len() != 2 {
                return Err(boxed_error("map entry must contain exactly two values"));
            }

            Ok((decode_hex(pair[0])?, pair[1].parse()?))
        })
        .collect()
}

fn write_u8_value(out: &mut Vec<u8>, value: &u8) -> HncsResult<()> {
    write_u8(out, *value);
    Ok(())
}

fn read_u8_value(decoder: &mut Decoder<'_>) -> HncsResult<u8> {
    decoder.read_u8()
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
    array(value)
}

fn value_at<'a>(mut value: &'a Value, path: &[&str]) -> Result<&'a Value, Box<dyn Error>> {
    for segment in path {
        value = value
            .get(*segment)
            .ok_or_else(|| boxed_error(format!("missing path {}", path_name(path))))?;
    }

    Ok(value)
}

fn array_field<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    let value = value
        .get(field)
        .ok_or_else(|| boxed_error(format!("missing array field {field}")))?;
    array(value)
}

fn string_array_field<'a>(value: &'a Value, field: &str) -> Result<Vec<&'a str>, Box<dyn Error>> {
    let value = value
        .get(field)
        .ok_or_else(|| boxed_error(format!("missing string array field {field}")))?;
    string_array(value)
}

fn array(value: &Value) -> Result<&[Value], Box<dyn Error>> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| boxed_error("value is not an array"))
}

fn string_array(value: &Value) -> Result<Vec<&str>, Box<dyn Error>> {
    array(value)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| boxed_error("array value is not a string"))
        })
        .collect()
}

fn str_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| boxed_error(format!("missing string field {field}")))
}

fn optional_string_field<'a>(
    value: &'a Value,
    field: &str,
) -> Result<Option<&'a str>, Box<dyn Error>> {
    let Some(value) = value.get(field) else {
        return Err(boxed_error(format!("missing optional field {field}")));
    };

    if value.is_null() {
        return Ok(None);
    }

    value
        .as_str()
        .map(Some)
        .ok_or_else(|| boxed_error(format!("field {field} is not a string or null")))
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
