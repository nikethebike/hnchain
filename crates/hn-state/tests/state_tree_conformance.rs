//! `hn-smt-256-v1` state tree conformance tests backed by language-independent
//! JSON vectors (ADR-0007).

use std::{error::Error, fmt, fs, path::PathBuf};

use hn_state::{
    EmptyHashTable, Leaf, StateError, compute_state_root, internal_hash, leaf_hash, state_key_core,
    state_key_extension, value_hash,
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
fn state_tree_vectors_are_conformant() -> Result<(), Box<dyn Error>> {
    let vectors = load_vectors()?;
    let empty_table = EmptyHashTable::build().map_err(state_error)?;

    verify_empty_hashes(&vectors, &empty_table)?;
    verify_state_keys(&vectors)?;
    verify_nodes(&vectors)?;
    verify_roots(&vectors, &empty_table)?;

    Ok(())
}

fn load_vectors() -> Result<Value, Box<dyn Error>> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("..");
    path.push("tests");
    path.push("conformance");
    path.push("core");
    path.push("state-tree-v0.1.json");

    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn verify_empty_hashes(root: &Value, empty_table: &EmptyHashTable) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["empty_hashes", "canonical"])? {
        let depth = usize_field(case, "depth")?;
        let expected = hex_field(case, "hex")?;
        assert_eq!(hex(&empty_table.get(depth)), hex(&expected));
    }

    Ok(())
}

fn verify_state_keys(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["state_keys", "canonical"])? {
        let expected = hex_field(case, "hex")?;
        let computed = compute_state_key(case)?.map_err(state_error)?;
        assert_eq!(hex(&computed), hex(&expected));
    }

    for case in array_at(root, &["state_keys", "invalid"])? {
        let expected_error = str_field(case, "error")?;
        let result = compute_state_key(case)?;
        assert_eq!(error_code(result), expected_error);
    }

    Ok(())
}

fn verify_nodes(root: &Value) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["nodes", "canonical"])? {
        let name = str_field(case, "name")?;
        let expected = hex_field(case, "hex")?;

        let computed = match name {
            "value-hash-envelope-account-a" => {
                value_hash(&hex_field(case, "value_hex")?).map_err(state_error)?
            }
            "leaf-hash-envelope-account-a" => {
                let state_key = digest_field(case, "state_key_hex")?;
                let value_hash = digest_field(case, "value_hash_hex")?;
                leaf_hash(&state_key, &value_hash).map_err(state_error)?
            }
            "internal-hash-of-empty-children" => {
                let left = digest_field(case, "left_hex")?;
                let right = digest_field(case, "right_hex")?;
                internal_hash(&left, &right).map_err(state_error)?
            }
            unsupported => return Err(boxed_error(format!("unsupported node case {unsupported}"))),
        };

        assert_eq!(hex(&computed), hex(&expected));
    }

    Ok(())
}

fn verify_roots(root: &Value, empty_table: &EmptyHashTable) -> Result<(), Box<dyn Error>> {
    for case in array_at(root, &["roots", "canonical"])? {
        let expected = hex_field(case, "hex")?;
        let leaves = leaves_field(case, "leaves")?;
        let computed = compute_state_root(&leaves, empty_table).map_err(state_error)?;
        assert_eq!(hex(&computed), hex(&expected));
    }

    for case in array_at(root, &["roots", "update_ordering"])? {
        let expected = hex_field(case, "hex")?;
        for semantic_input in array_field(case, "semantic_inputs")? {
            let leaves = leaves(semantic_input)?;
            let computed = compute_state_root(&leaves, empty_table).map_err(state_error)?;
            assert_eq!(hex(&computed), hex(&expected));
        }
    }

    for case in array_at(root, &["roots", "invalid"])? {
        let expected_error = str_field(case, "error")?;
        let leaves = leaves_field(case, "leaves")?;
        let result = compute_state_root(&leaves, empty_table);
        assert_eq!(error_code(result.map(|_| ())), expected_error);
    }

    Ok(())
}

/// Returns `Ok(Err(StateError))` for a case that failed to derive a key
/// (an "invalid" case), and `Ok(Ok(key))` otherwise, so callers can decide
/// how to treat the result without this helper itself asserting anything.
fn compute_state_key(case: &Value) -> Result<Result<[u8; 32], StateError>, Box<dyn Error>> {
    let kind = str_field(case, "kind")?;
    let domain_id = u8_field(case, "domain_id")?;
    let object_id = hex_field(case, "object_id_hex")?;
    let subkey = hex_field(case, "subkey_hex")?;

    Ok(match kind {
        "core" => {
            let section_id = u8_field(case, "section_id")?;
            state_key_core(domain_id, section_id, &object_id, &subkey)
        }
        "extension" => {
            let extension_id = u16_field(case, "extension_id")?;
            state_key_extension(domain_id, extension_id, &object_id, &subkey)
        }
        unsupported => {
            return Err(boxed_error(format!(
                "unsupported state key kind {unsupported}"
            )));
        }
    })
}

fn leaves_field(value: &Value, field: &str) -> Result<Vec<Leaf>, Box<dyn Error>> {
    leaves(
        value
            .get(field)
            .ok_or_else(|| boxed_error(format!("missing array field {field}")))?,
    )
}

fn leaves(value: &Value) -> Result<Vec<Leaf>, Box<dyn Error>> {
    array(value)?
        .iter()
        .map(|entry| {
            let state_key = digest_field(entry, "state_key_hex")?;
            let value = hex_field(entry, "value_hex")?;
            let vh = value_hash(&value).map_err(state_error)?;
            let lh = leaf_hash(&state_key, &vh).map_err(state_error)?;
            Ok((state_key, lh))
        })
        .collect()
}

fn error_code<T>(result: Result<T, StateError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(StateError::DuplicateStateKey) => "duplicate_state_key",
        Err(StateError::Hash(hn_crypto::HashError::Framing(
            hn_hncs::HncsError::LengthLimitExceeded { .. },
        ))) => "length_limit_exceeded",
        Err(_) => "error",
    }
}

fn state_error(error: StateError) -> Box<dyn Error> {
    boxed_error(error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digest_field(value: &Value, field: &str) -> Result<[u8; 32], Box<dyn Error>> {
    let bytes = hex_field(value, field)?;
    <[u8; 32]>::try_from(bytes).map_err(|bytes| {
        boxed_error(format!(
            "field {field} is not 32 bytes, got {}",
            bytes.len()
        ))
    })
}

fn array_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a [Value], Box<dyn Error>> {
    let mut current = value;
    for segment in path {
        current = current
            .get(*segment)
            .ok_or_else(|| boxed_error(format!("missing path {}", path.join("."))))?;
    }
    array(current)
}

fn array_field<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    let value = value
        .get(field)
        .ok_or_else(|| boxed_error(format!("missing array field {field}")))?;
    array(value)
}

fn array(value: &Value) -> Result<&[Value], Box<dyn Error>> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| boxed_error("value is not an array"))
}

fn str_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| boxed_error(format!("missing string field {field}")))
}

fn usize_field(value: &Value, field: &str) -> Result<usize, Box<dyn Error>> {
    let number = value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| boxed_error(format!("missing usize field {field}")))?;
    Ok(usize::try_from(number)?)
}

fn u8_field(value: &Value, field: &str) -> Result<u8, Box<dyn Error>> {
    let number = value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| boxed_error(format!("missing u8 field {field}")))?;
    Ok(u8::try_from(number)?)
}

fn u16_field(value: &Value, field: &str) -> Result<u16, Box<dyn Error>> {
    let number = value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| boxed_error(format!("missing u16 field {field}")))?;
    Ok(u16::try_from(number)?)
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
