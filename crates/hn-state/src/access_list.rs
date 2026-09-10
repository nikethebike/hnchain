use hn_crypto::Digest;
use hn_hncs::{Decoder, write_fixed_bytes, write_set};

use crate::error::{StateError, StateResult};

/// Maximum number of entries in either `reads` or `writes`. An
/// implementation DoS bound, not derived — meaningful only because
/// `AccessListV1` is hint-only (ADR-0006, "Access List"): the cap bounds
/// processing cost, never correctness.
pub const MAX_ACCESS_LIST_ENTRIES: usize = 256;

/// The `TransactionEnvelope.access_list` field (ADR-0006, "Access
/// List"): a hint, never consensus-enforced. Each entry is a
/// `state_key` (ADR-0007) — the same 32-byte value the state tree
/// already uses as its one uniform leaf address for every domain, so
/// entries need no domain-specific structure. A key may legitimately
/// appear in both `reads` and `writes` (a read-modify-write is not a
/// conflict with itself).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessListV1 {
    /// State keys this transaction expects to read.
    pub reads: Vec<Digest>,
    /// State keys this transaction expects to write.
    pub writes: Vec<Digest>,
}

impl AccessListV1 {
    /// Encodes this value as canonical HNCS bytes (ADR-0006, "Access
    /// List"): each of `reads`/`writes` is an independent bounded
    /// canonical set (HNCS `set`, ADR-0004 — sorted by encoded bytes,
    /// duplicates rejected).
    pub fn encode(&self) -> StateResult<Vec<u8>> {
        let mut out = Vec::new();
        write_set(
            &mut out,
            &self.reads,
            MAX_ACCESS_LIST_ENTRIES,
            |out, key| {
                write_fixed_bytes(out, key);
                Ok(())
            },
        )
        .map_err(StateError::Encoding)?;
        write_set(
            &mut out,
            &self.writes,
            MAX_ACCESS_LIST_ENTRIES,
            |out, key| {
                write_fixed_bytes(out, key);
                Ok(())
            },
        )
        .map_err(StateError::Encoding)?;
        Ok(out)
    }

    /// Decodes and validates canonical HNCS bytes produced by
    /// [`AccessListV1::encode`].
    pub fn decode(bytes: &[u8]) -> StateResult<Self> {
        let mut decoder = Decoder::new(bytes);

        let reads = decoder
            .read_set(MAX_ACCESS_LIST_ENTRIES, |decoder| {
                decoder.read_fixed_bytes::<32>()
            })
            .map_err(StateError::Encoding)?;
        let writes = decoder
            .read_set(MAX_ACCESS_LIST_ENTRIES, |decoder| {
                decoder.read_fixed_bytes::<32>()
            })
            .map_err(StateError::Encoding)?;
        decoder.finish().map_err(StateError::Encoding)?;

        Ok(Self { reads, writes })
    }
}

#[cfg(test)]
mod tests {
    use super::AccessListV1;

    const R1: [u8; 32] = [0x01; 32];
    const R2: [u8; 32] = [0x02; 32];
    const W1: [u8; 32] = [0x03; 32];

    #[test]
    fn encodes_matching_independent_oracle() -> crate::error::StateResult<()> {
        // Constructed out of canonical order on purpose: encoding must
        // sort by encoded bytes regardless of input order.
        let access_list = AccessListV1 {
            reads: vec![R2, R1],
            writes: vec![W1],
        };
        assert_eq!(
            hex(&access_list.encode()?),
            "02000000\
             0101010101010101010101010101010101010101010101010101010101010101\
             0202020202020202020202020202020202020202020202020202020202020202\
             01000000\
             0303030303030303030303030303030303030303030303030303030303030303"
        );
        Ok(())
    }

    #[test]
    fn encodes_empty_access_list() -> crate::error::StateResult<()> {
        let access_list = AccessListV1 {
            reads: vec![],
            writes: vec![],
        };
        assert_eq!(hex(&access_list.encode()?), "0000000000000000");
        Ok(())
    }

    #[test]
    fn round_trips_through_decode() -> crate::error::StateResult<()> {
        let access_list = AccessListV1 {
            reads: vec![R1, R2],
            writes: vec![W1, R1],
        };
        let mut decoded = AccessListV1::decode(&access_list.encode()?)?;
        decoded.reads.sort();
        decoded.writes.sort();
        let mut expected = access_list;
        expected.reads.sort();
        expected.writes.sort();
        assert_eq!(decoded, expected);
        Ok(())
    }

    #[test]
    fn rejects_duplicate_entries_within_one_set() {
        let access_list = AccessListV1 {
            reads: vec![R1, R1],
            writes: vec![],
        };
        assert!(access_list.encode().is_err());
    }

    #[test]
    fn allows_same_key_in_reads_and_writes() -> crate::error::StateResult<()> {
        let access_list = AccessListV1 {
            reads: vec![R1],
            writes: vec![R1],
        };
        assert!(access_list.encode().is_ok());
        Ok(())
    }

    #[test]
    fn rejects_trailing_bytes() -> crate::error::StateResult<()> {
        let access_list = AccessListV1 {
            reads: vec![],
            writes: vec![],
        };
        let mut encoded = access_list.encode()?;
        encoded.push(0xff);
        assert!(AccessListV1::decode(&encoded).is_err());
        Ok(())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
