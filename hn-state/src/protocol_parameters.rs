use hn_crypto::{Digest, hash_profile_0x0001};

use crate::error::StateResult;

/// Computes `protocol_parameters_hash`'s genesis/placeholder value
/// (ADR-0008, "Decided: Genesis Protocol Parameters Hash
/// (Placeholder)"):
///
/// `HASH_PROFILE_0x0001("hnchain.protocol.parameters.v1", <empty bytes>)`
///
/// Not a commitment to any real, named protocol parameter — none has
/// been decided anywhere in this project yet (ADR-0008's own "Open
/// Decisions": "no adjustable parameter has been named anywhere yet").
/// A hash over nothing, exactly matching the field's own honestly-empty
/// state, reserved for real content once a parameter-commitment format
/// is eventually decided — not an invented stand-in value picked ad
/// hoc. Every block before that format exists (genesis included) uses
/// this same value.
pub fn protocol_parameters_placeholder_hash() -> StateResult<Digest> {
    Ok(hash_profile_0x0001("hnchain.protocol.parameters.v1", &[])?)
}

#[cfg(test)]
mod tests {
    use super::protocol_parameters_placeholder_hash;
    use crate::error::StateResult;

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        assert_eq!(
            hex(&protocol_parameters_placeholder_hash()?),
            "52dfa11b68014f278d1a7ce77a98dbf19c21bff1531b1a98921d9cc23ff9c911"
        );
        Ok(())
    }

    #[test]
    fn is_deterministic() -> StateResult<()> {
        assert_eq!(
            protocol_parameters_placeholder_hash()?,
            protocol_parameters_placeholder_hash()?
        );
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
