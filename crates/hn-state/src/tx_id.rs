use hn_crypto::{Digest, hash_profile_0x0001};

use crate::error::StateResult;

/// Computes a transaction ID (ADR-0006, "Transaction ID"):
///
/// `tx_id = HASH_PROFILE_0x0001("hnchain.transaction.id.v1", HNCS(TransactionEnvelope))`
///
/// `envelope_bytes` is expected to already be the canonical HNCS
/// encoding of `TransactionEnvelope`; this crate does not define a
/// concrete `TransactionEnvelope` schema (several of its fields are not
/// decided yet), matching how [`crate::block_hash`] and
/// [`crate::value_hash`] treat their own input as already-canonical
/// bytes rather than defining the schema itself.
pub fn tx_id(envelope_bytes: &[u8]) -> StateResult<Digest> {
    Ok(hash_profile_0x0001(
        "hnchain.transaction.id.v1",
        envelope_bytes,
    )?)
}

#[cfg(test)]
mod tests {
    use super::tx_id;
    use crate::error::StateResult;

    #[test]
    fn matches_independent_oracle() -> StateResult<()> {
        let envelope_bytes = b"canonical-tx-envelope-placeholder-v1";
        assert_eq!(
            hex(&tx_id(envelope_bytes)?),
            "5fce01099930a3c46e91ee0a4563f0f939182737f538882df94814136e7bee11"
        );
        Ok(())
    }

    #[test]
    fn distinct_envelopes_hash_differently() -> StateResult<()> {
        assert_ne!(tx_id(b"envelope-a")?, tx_id(b"envelope-b")?);
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
