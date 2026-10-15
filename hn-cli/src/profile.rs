//! The active network profile (ADR-0047, "Decided: Network Pinning Via
//! A Profile"). Pins this CLI build's own `devnet` identity --
//! `chain_id`, `network_id`, `genesis_hash`, the default RPC URL, and
//! the fee/window defaults -- so a mistyped or hostile RPC endpoint
//! cannot get a transaction signed for the wrong network:
//! [`validate_status`] refuses when a node's own `GET /v1/status`
//! disagrees with these pinned values.
//!
//! `DEFAULT_FEE_LIMIT_HNIT` and `VALIDITY_WINDOW_BLOCKS` are explicit
//! placeholder constants, not protocol policy (ADR-0047, "Fee limit
//! and validity window": the fee model is an undecided economic
//! parameter, ADR-0030). Both are printed as placeholders in every
//! `send` confirmation summary, never silently assumed. `DEFAULT_RPC_URL`
//! is likewise a CLI-only convention: ADR-0046 fixes no port, since RPC
//! is off by default and the bind address is the operator's own choice.
//!
//! The CLI ships this one `devnet` profile only; no mainnet or testnet
//! profile exists until those networks do.

use std::fmt;

use hn_core::ChainId;

/// The devnet's own pinned `chain_id` (ADR-0006, "Chain And Network
/// Binding").
pub const CHAIN_ID: u8 = ChainId::HNCHAIN.get();

/// The devnet's own pinned `network_id` (ADR-0003).
pub const NETWORK_ID: u16 = 1;

/// The committed `hn-node/genesis/devnet.json`'s own real
/// `genesis_hash` (ADR-0038, "Decided: Genesis Hash") -- the node's
/// own `GenesisManifest::genesis_hash()` output for that file, not
/// re-derived independently here.
pub const GENESIS_HASH: [u8; 32] = [
    0xa3, 0x75, 0x72, 0x77, 0xb3, 0x8e, 0xe0, 0x01, 0xf3, 0xce, 0x2e, 0xcd, 0xb2, 0xc3, 0x0e, 0x0b,
    0x27, 0x9e, 0xf3, 0xd8, 0xc3, 0x30, 0xeb, 0x87, 0x82, 0x35, 0x68, 0xab, 0x6c, 0x8c, 0xb3, 0xdc,
];

/// `--rpc`'s own default when not given.
pub const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8080";

/// Placeholder default for `--fee-limit`, in `hnit`.
pub const DEFAULT_FEE_LIMIT_HNIT: u128 = 100;

/// Placeholder default validity-window length, in blocks:
/// `max_height = latest_height + VALIDITY_WINDOW_BLOCKS` unless
/// `--no-expiry` is given.
pub const VALIDITY_WINDOW_BLOCKS: u64 = 100;

/// Why a node's own reported status disagrees with this pinned
/// profile. A caller about to sign a transaction must abort on this,
/// never proceed (ADR-0047).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileMismatch {
    /// The node's own `chain_id` does not match [`CHAIN_ID`].
    ChainId {
        /// This profile's own pinned value.
        expected: u8,
        /// What the node actually reported.
        actual: u8,
    },
    /// The node's own `network_id` does not match [`NETWORK_ID`].
    NetworkId {
        /// This profile's own pinned value.
        expected: u16,
        /// What the node actually reported.
        actual: u16,
    },
    /// The node's own `genesis_hash` does not match [`GENESIS_HASH`].
    GenesisHash {
        /// This profile's own pinned value.
        expected: [u8; 32],
        /// What the node actually reported.
        actual: [u8; 32],
    },
}

impl fmt::Display for ProfileMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChainId { expected, actual } => write!(
                formatter,
                "node reports chain_id {actual}, this profile expects {expected}"
            ),
            Self::NetworkId { expected, actual } => write!(
                formatter,
                "node reports network_id {actual}, this profile expects {expected}"
            ),
            Self::GenesisHash { expected, actual } => write!(
                formatter,
                "node reports genesis_hash {}, this profile expects {}",
                hex(actual),
                hex(expected)
            ),
        }
    }
}

impl std::error::Error for ProfileMismatch {}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Checks a node's own `GET /v1/status` fields against this pinned
/// devnet profile. `Err` means the caller must refuse to sign anything
/// against this node (ADR-0047, "Decided: Network Pinning Via A
/// Profile").
pub fn validate_status(
    chain_id: u8,
    network_id: u16,
    genesis_hash: [u8; 32],
) -> Result<(), ProfileMismatch> {
    if chain_id != CHAIN_ID {
        return Err(ProfileMismatch::ChainId {
            expected: CHAIN_ID,
            actual: chain_id,
        });
    }
    if network_id != NETWORK_ID {
        return Err(ProfileMismatch::NetworkId {
            expected: NETWORK_ID,
            actual: network_id,
        });
    }
    if genesis_hash != GENESIS_HASH {
        return Err(ProfileMismatch::GenesisHash {
            expected: GENESIS_HASH,
            actual: genesis_hash,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CHAIN_ID, GENESIS_HASH, NETWORK_ID, ProfileMismatch, validate_status};

    #[test]
    fn accepts_the_pinned_values() {
        assert_eq!(validate_status(CHAIN_ID, NETWORK_ID, GENESIS_HASH), Ok(()));
    }

    #[test]
    fn rejects_a_mismatched_chain_id() {
        assert_eq!(
            validate_status(CHAIN_ID.wrapping_add(1), NETWORK_ID, GENESIS_HASH),
            Err(ProfileMismatch::ChainId {
                expected: CHAIN_ID,
                actual: CHAIN_ID.wrapping_add(1),
            })
        );
    }

    #[test]
    fn rejects_a_mismatched_network_id() {
        assert_eq!(
            validate_status(CHAIN_ID, NETWORK_ID.wrapping_add(1), GENESIS_HASH),
            Err(ProfileMismatch::NetworkId {
                expected: NETWORK_ID,
                actual: NETWORK_ID.wrapping_add(1),
            })
        );
    }

    #[test]
    fn rejects_a_mismatched_genesis_hash() {
        let mut wrong = GENESIS_HASH;
        wrong[0] ^= 0xFF;
        assert_eq!(
            validate_status(CHAIN_ID, NETWORK_ID, wrong),
            Err(ProfileMismatch::GenesisHash {
                expected: GENESIS_HASH,
                actual: wrong,
            })
        );
    }

    #[test]
    fn checks_chain_id_before_network_id_or_genesis_hash() {
        // A node that disagrees on more than one field still reports a
        // single, deterministic mismatch -- chain_id first, since it
        // is the outermost binding (ADR-0006).
        let mut wrong_hash = GENESIS_HASH;
        wrong_hash[0] ^= 0xFF;
        assert_eq!(
            validate_status(
                CHAIN_ID.wrapping_add(1),
                NETWORK_ID.wrapping_add(1),
                wrong_hash
            ),
            Err(ProfileMismatch::ChainId {
                expected: CHAIN_ID,
                actual: CHAIN_ID.wrapping_add(1),
            })
        );
    }
}
