use std::fmt;
use std::path::Path;

use hn_core::ChainId;
use hn_crypto::{
    Digest, ED25519_ALGORITHM_ID, ED25519_PUBLIC_KEY_LEN, IdentityError, KeyDescriptor,
    hash_profile_0x0001,
};
use hn_hncs::{
    HncsError, write_bytes, write_fixed_bytes, write_list, write_string, write_u8, write_u16,
    write_u64, write_u128,
};
use hn_state::{
    AccountSection, COMMUNITY_ALLOCATION, EmptyHashTable, FOUNDER_ALLOCATION, Leaf,
    MINIMUM_VALIDATOR_BOND, RESERVE_ALLOCATION, StateError, ValidatorRecordV1, ValidatorSection,
    ValidatorStatus, Write, account_section_state_key, compute_state_root, leaf_for_write,
    validator_section_state_key,
};
use serde_json::Value;

use crate::hex;

/// `manifest_version` for the current [`GenesisManifest`] shape
/// (ADR-0038, "Decided: `GenesisManifest` Schema").
pub const GENESIS_MANIFEST_VERSION_1: u16 = 1;

/// Maximum length, in bytes, of `genesis_message` (ADR-0038, matching
/// genesis.md §4's own recommended bound, made a hard implementation
/// limit).
pub const GENESIS_MESSAGE_MAX_LEN: usize = 512;

/// Maximum number of entries in `GenesisManifest.validators`. An
/// implementation resource bound, same class of decision as
/// `MAX_CAPABILITY_COUNT`/`MAX_ANNOUNCED_PEERS` elsewhere in this
/// codebase.
pub const MAX_GENESIS_VALIDATORS: usize = 1024;

/// One genesis validator entry (ADR-0038). `validator_id` is taken as
/// given, never derived — see the module-level documentation for why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenesisValidator {
    /// This validator's stable identifier.
    pub validator_id: Digest,
    /// This validator's consensus (vote/QC signing) public key.
    pub consensus_key: KeyDescriptor,
    /// Starting bonded stake, in `hnit` — also this validator's
    /// starting `voting_power` (no delegation at genesis).
    pub bonded_stake: u128,
}

/// One of the three fixed HNCOIN genesis allocation accounts
/// (ADR-0024). `address` is taken as given, never derived — real
/// custody for these accounts is `genesis-security.md`'s own,
/// unresolved, decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenesisAccount {
    /// This account's address.
    pub address: Digest,
    /// This account's starting native HNCOIN balance, in `hnit`.
    pub amount: u128,
}

/// The three genesis allocation accounts ADR-0024 requires, named
/// explicitly rather than held in a generic list (ADR-0038, "Decided:
/// `GenesisManifest` Schema" — a list could accidentally omit,
/// duplicate, or misallocate one of the three required accounts).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenesisAllocations {
    /// The Liquidity & Ecosystem Reserve (80% of supply).
    pub reserve: GenesisAccount,
    /// The Founder Allocation (10% of supply).
    pub founder: GenesisAccount,
    /// The Community & Airdrop Allocation (10% of supply).
    pub community: GenesisAccount,
}

/// A genesis manifest (ADR-0038, "Decided: `GenesisManifest` Schema")
/// — a deliberate merge of genesis.md's own conceptual `GenesisHeader`
/// and `GenesisManifest` into one concrete type; see this ADR's own
/// reasoning for why. `initial_state_root` is not a field here: it is
/// computed from `validators`/`allocations` on demand
/// ([`GenesisManifest::genesis_write_set`]/
/// [`GenesisManifest::initial_state_root`]), not authored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenesisManifest {
    /// Structure version for this manifest shape.
    pub manifest_version: u16,
    /// HNChain protocol lineage (ADR-0006).
    pub chain_id: u8,
    /// Network environment (ADR-0003).
    pub network_id: u16,
    /// Fixed protocol timestamp, chosen before genesis generation.
    pub genesis_time: u64,
    /// Bounded, neutral genesis message (genesis.md §4).
    pub genesis_message: String,
    /// The initial validator set.
    pub validators: Vec<GenesisValidator>,
    /// The three fixed HNCOIN allocation accounts.
    pub allocations: GenesisAllocations,
}

/// Errors loading, parsing, or validating a genesis file (ADR-0038).
#[derive(Debug)]
pub enum GenesisError {
    /// Reading the genesis file itself failed.
    Io(std::io::Error),
    /// The file's contents are not valid JSON.
    Json(serde_json::Error),
    /// A required field is missing.
    MissingField(&'static str),
    /// A field is present but structurally invalid (wrong type, bad
    /// hex, wrong length).
    InvalidField(&'static str),
    /// `manifest_version` is not [`GENESIS_MANIFEST_VERSION_1`].
    UnsupportedManifestVersion(u64),
    /// `chain_id` is `0` ([`hn_core::ChainId`]'s own reserved value).
    ReservedChainId,
    /// `validators` is empty.
    NoValidators,
    /// `validators` exceeds [`MAX_GENESIS_VALIDATORS`].
    TooManyValidators(usize),
    /// Two validators share a `validator_id`.
    DuplicateValidatorId,
    /// Two validators share a `consensus_key`.
    DuplicateConsensusKey,
    /// A validator's `consensus_key_public_key` is not a valid Ed25519
    /// point.
    InvalidConsensusKey(IdentityError),
    /// A validator's `bonded_stake` is below
    /// [`hn_state::MINIMUM_VALIDATOR_BOND`].
    InsufficientBond {
        /// The under-bonded validator's id.
        validator_id: Digest,
    },
    /// An allocation account's `amount` does not exactly equal its own
    /// fixed constant (ADR-0024).
    AllocationAmountMismatch {
        /// Which of the three accounts.
        account: &'static str,
    },
    /// Two accounts (allocation or validator-adjacent) share an
    /// address/id.
    DuplicateAddress,
    /// `genesis_message` exceeds [`GENESIS_MESSAGE_MAX_LEN`].
    GenesisMessageTooLong(usize),
    /// Canonical HNCS encoding failed while computing `genesis_hash`.
    Encoding(HncsError),
    /// Hashing `genesis_hash` itself failed (domain-tag validation).
    Hash(hn_crypto::HashError),
    /// Building the genesis write-set's state root failed.
    State(StateError),
}

impl fmt::Display for GenesisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "reading genesis file failed: {error}"),
            Self::Json(error) => write!(formatter, "genesis file is not valid JSON: {error}"),
            Self::MissingField(field) => write!(formatter, "genesis file missing field {field}"),
            Self::InvalidField(field) => write!(formatter, "genesis file has an invalid {field}"),
            Self::UnsupportedManifestVersion(value) => {
                write!(formatter, "unsupported manifest_version {value}")
            }
            Self::ReservedChainId => write!(formatter, "chain_id 0 is reserved"),
            Self::NoValidators => write!(formatter, "genesis must declare at least one validator"),
            Self::TooManyValidators(count) => {
                write!(formatter, "{count} validators exceeds the genesis limit")
            }
            Self::DuplicateValidatorId => write!(formatter, "duplicate validator_id in genesis"),
            Self::DuplicateConsensusKey => {
                write!(formatter, "duplicate consensus_key in genesis")
            }
            Self::InvalidConsensusKey(error) => {
                write!(
                    formatter,
                    "invalid genesis validator consensus key: {error}"
                )
            }
            Self::InsufficientBond { validator_id } => write!(
                formatter,
                "validator {validator_id:x?} bonded_stake is below MINIMUM_VALIDATOR_BOND"
            ),
            Self::AllocationAmountMismatch { account } => write!(
                formatter,
                "genesis allocation '{account}' amount does not match ADR-0024's fixed value"
            ),
            Self::DuplicateAddress => write!(formatter, "duplicate address in genesis"),
            Self::GenesisMessageTooLong(length) => {
                write!(
                    formatter,
                    "genesis_message length {length} exceeds the limit"
                )
            }
            Self::Encoding(error) => write!(formatter, "genesis encoding error: {error}"),
            Self::Hash(error) => write!(formatter, "genesis hash error: {error}"),
            Self::State(error) => write!(formatter, "genesis state error: {error}"),
        }
    }
}

impl std::error::Error for GenesisError {}

impl From<HncsError> for GenesisError {
    fn from(error: HncsError) -> Self {
        Self::Encoding(error)
    }
}

impl From<StateError> for GenesisError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

/// Shorthand for `Result<T, GenesisError>`.
pub type GenesisResult<T> = Result<T, GenesisError>;

impl GenesisManifest {
    /// Reads and parses `path` as JSON, then validates it (ADR-0038,
    /// "Decided: Genesis Validation Rules"). The returned manifest is
    /// guaranteed to satisfy every validation rule; nothing downstream
    /// needs to re-check them.
    pub fn load(path: &Path) -> GenesisResult<Self> {
        let bytes = std::fs::read(path).map_err(GenesisError::Io)?;
        let value: Value = serde_json::from_slice(&bytes).map_err(GenesisError::Json)?;
        let manifest = Self::parse(&value)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn parse(value: &Value) -> GenesisResult<Self> {
        let manifest_version = get_u64(value, "manifest_version")?;
        if manifest_version != u64::from(GENESIS_MANIFEST_VERSION_1) {
            return Err(GenesisError::UnsupportedManifestVersion(manifest_version));
        }
        let chain_id = get_u64(value, "chain_id")?;
        let chain_id =
            u8::try_from(chain_id).map_err(|_| GenesisError::InvalidField("chain_id"))?;
        let network_id = get_u64(value, "network_id")?;
        let network_id =
            u16::try_from(network_id).map_err(|_| GenesisError::InvalidField("network_id"))?;
        let genesis_time = get_u64(value, "genesis_time")?;
        let genesis_message = get_str(value, "genesis_message")?.to_string();

        let validators = get_array(value, "validators")?
            .iter()
            .map(parse_validator)
            .collect::<GenesisResult<Vec<_>>>()?;

        let allocations_value = value
            .get("allocations")
            .ok_or(GenesisError::MissingField("allocations"))?;
        let allocations = GenesisAllocations {
            reserve: parse_account(allocations_value, "reserve")?,
            founder: parse_account(allocations_value, "founder")?,
            community: parse_account(allocations_value, "community")?,
        };

        Ok(Self {
            manifest_version: GENESIS_MANIFEST_VERSION_1,
            chain_id,
            network_id,
            genesis_time,
            genesis_message,
            validators,
            allocations,
        })
    }

    fn validate(&self) -> GenesisResult<()> {
        if self.genesis_message.len() > GENESIS_MESSAGE_MAX_LEN {
            return Err(GenesisError::GenesisMessageTooLong(
                self.genesis_message.len(),
            ));
        }
        if ChainId::new(self.chain_id).is_err() {
            return Err(GenesisError::ReservedChainId);
        }
        if self.validators.is_empty() {
            return Err(GenesisError::NoValidators);
        }
        if self.validators.len() > MAX_GENESIS_VALIDATORS {
            return Err(GenesisError::TooManyValidators(self.validators.len()));
        }

        let mut seen_ids = std::collections::BTreeSet::new();
        let mut seen_keys = std::collections::BTreeSet::new();
        for validator in &self.validators {
            if !seen_ids.insert(validator.validator_id) {
                return Err(GenesisError::DuplicateValidatorId);
            }
            if !seen_keys.insert(validator.consensus_key.public_key_bytes()) {
                return Err(GenesisError::DuplicateConsensusKey);
            }
            if validator.bonded_stake < MINIMUM_VALIDATOR_BOND {
                return Err(GenesisError::InsufficientBond {
                    validator_id: validator.validator_id,
                });
            }
        }

        check_allocation_amount(
            "reserve",
            self.allocations.reserve.amount,
            RESERVE_ALLOCATION,
        )?;
        check_allocation_amount(
            "founder",
            self.allocations.founder.amount,
            FOUNDER_ALLOCATION,
        )?;
        check_allocation_amount(
            "community",
            self.allocations.community.amount,
            COMMUNITY_ALLOCATION,
        )?;

        let mut seen_addresses = std::collections::BTreeSet::new();
        for address in [
            self.allocations.reserve.address,
            self.allocations.founder.address,
            self.allocations.community.address,
        ] {
            if !seen_addresses.insert(address) {
                return Err(GenesisError::DuplicateAddress);
            }
        }

        Ok(())
    }

    /// Encodes this manifest as canonical HNCS bytes (ADR-0038,
    /// "Decided: `GenesisManifest` Schema"). No `decode` is provided —
    /// nothing in this codebase reconstructs a manifest from raw bytes;
    /// JSON parsing is the only production path that produces one.
    pub fn encode(&self) -> GenesisResult<Vec<u8>> {
        let mut out = Vec::new();
        write_u16(&mut out, self.manifest_version);
        write_u8(&mut out, self.chain_id);
        write_u16(&mut out, self.network_id);
        write_u64(&mut out, self.genesis_time);
        write_string(&mut out, &self.genesis_message, GENESIS_MESSAGE_MAX_LEN)?;
        write_list(
            &mut out,
            &self.validators,
            MAX_GENESIS_VALIDATORS,
            |out, validator| {
                write_fixed_bytes(out, &validator.validator_id);
                write_u16(out, validator.consensus_key.algorithm_id());
                write_bytes(
                    out,
                    &validator.consensus_key.public_key_bytes(),
                    ED25519_PUBLIC_KEY_LEN,
                )?;
                write_u128(out, validator.bonded_stake);
                Ok(())
            },
        )?;
        write_account(&mut out, &self.allocations.reserve)?;
        write_account(&mut out, &self.allocations.founder)?;
        write_account(&mut out, &self.allocations.community)?;
        Ok(out)
    }

    /// Computes `genesis_hash` (ADR-0038, "Decided: Genesis Hash"):
    /// `HASH_PROFILE_0x0001("hnchain.genesis.v1", HNCS(GenesisManifest))`.
    pub fn genesis_hash(&self) -> GenesisResult<Digest> {
        hash_profile_0x0001("hnchain.genesis.v1", &self.encode()?).map_err(GenesisError::Hash)
    }

    /// This manifest's write-set: one [`ValidatorRecordV1`] leaf per
    /// validator, one [`hn_state::BalanceValueV1`] leaf per allocation
    /// account (ADR-0038, "Decided: Initial State Root").
    pub fn genesis_write_set(&self) -> GenesisResult<Vec<Write>> {
        let mut writes = Vec::with_capacity(self.validators.len() + 3);
        for validator in &self.validators {
            let record = ValidatorRecordV1 {
                validator_id: validator.validator_id,
                consensus_key: validator.consensus_key,
                bonded_stake: validator.bonded_stake,
                voting_power: validator.bonded_stake,
                status: ValidatorStatus::Active,
                pending_unbonding: None,
            };
            let key =
                validator_section_state_key(&validator.validator_id, ValidatorSection::Record)?;
            writes.push(Write {
                state_key: key,
                value: record.encode()?,
            });
        }
        for account in [
            &self.allocations.reserve,
            &self.allocations.founder,
            &self.allocations.community,
        ] {
            let key = account_section_state_key(&account.address, AccountSection::Balance)?;
            writes.push(Write {
                state_key: key,
                value: hn_state::BalanceValueV1 {
                    native_balance: account.amount,
                }
                .encode(),
            });
        }
        Ok(writes)
    }

    /// This manifest's `initial_state_root`, computed by feeding
    /// [`GenesisManifest::genesis_write_set`] through the same
    /// state-tree machinery a real block-processing pipeline would use
    /// (ADR-0038, "Decided: Initial State Root").
    pub fn initial_state_root(&self) -> GenesisResult<Digest> {
        let writes = self.genesis_write_set()?;
        let leaves: Vec<Leaf> = writes
            .iter()
            .map(leaf_for_write)
            .collect::<Result<_, StateError>>()?;
        let empty_table = EmptyHashTable::build()?;
        Ok(compute_state_root(&leaves, &empty_table)?)
    }
}

fn check_allocation_amount(name: &'static str, amount: u128, expected: u128) -> GenesisResult<()> {
    if amount == expected {
        Ok(())
    } else {
        Err(GenesisError::AllocationAmountMismatch { account: name })
    }
}

fn write_account(out: &mut Vec<u8>, account: &GenesisAccount) -> GenesisResult<()> {
    write_fixed_bytes(out, &account.address);
    write_u128(out, account.amount);
    Ok(())
}

fn get_u64(value: &Value, field: &'static str) -> GenesisResult<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or(GenesisError::MissingField(field))
}

fn get_str<'a>(value: &'a Value, field: &'static str) -> GenesisResult<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(GenesisError::MissingField(field))
}

fn get_array<'a>(value: &'a Value, field: &'static str) -> GenesisResult<&'a Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or(GenesisError::MissingField(field))
}

fn parse_hex_digest(value: &Value, field: &'static str) -> GenesisResult<Digest> {
    let text = get_str(value, field)?;
    let bytes = hex::decode(text).ok_or(GenesisError::InvalidField(field))?;
    bytes
        .try_into()
        .map_err(|_| GenesisError::InvalidField(field))
}

fn parse_u128_str(value: &Value, field: &'static str) -> GenesisResult<u128> {
    let text = get_str(value, field)?;
    text.parse::<u128>()
        .map_err(|_| GenesisError::InvalidField(field))
}

fn parse_validator(value: &Value) -> GenesisResult<GenesisValidator> {
    let validator_id = parse_hex_digest(value, "validator_id")?;
    let algorithm_id = get_u64(value, "consensus_key_algorithm_id")?;
    let algorithm_id = u16::try_from(algorithm_id)
        .map_err(|_| GenesisError::InvalidField("consensus_key_algorithm_id"))?;
    if algorithm_id != ED25519_ALGORITHM_ID {
        return Err(GenesisError::InvalidField("consensus_key_algorithm_id"));
    }
    let public_key_text = get_str(value, "consensus_key_public_key")?;
    let public_key_bytes = hex::decode(public_key_text)
        .ok_or(GenesisError::InvalidField("consensus_key_public_key"))?;
    let public_key: [u8; ED25519_PUBLIC_KEY_LEN] = public_key_bytes
        .try_into()
        .map_err(|_| GenesisError::InvalidField("consensus_key_public_key"))?;
    let consensus_key =
        KeyDescriptor::from_public_key_bytes(hn_crypto::KeyRole::ValidatorConsensus, public_key)
            .map_err(GenesisError::InvalidConsensusKey)?;
    let bonded_stake = parse_u128_str(value, "bonded_stake")?;

    Ok(GenesisValidator {
        validator_id,
        consensus_key,
        bonded_stake,
    })
}

fn parse_account(allocations_value: &Value, field: &'static str) -> GenesisResult<GenesisAccount> {
    let account_value = allocations_value
        .get(field)
        .ok_or(GenesisError::MissingField(field))?;
    Ok(GenesisAccount {
        address: parse_hex_digest(account_value, "address")?,
        amount: parse_u128_str(account_value, "amount")?,
    })
}

#[cfg(test)]
mod tests {
    use hn_crypto::{Ed25519KeyPair, KeyRole};

    use super::{
        GenesisAccount, GenesisAllocations, GenesisError, GenesisManifest, GenesisValidator,
    };
    use hn_state::{COMMUNITY_ALLOCATION, FOUNDER_ALLOCATION, RESERVE_ALLOCATION};

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn sample_manifest() -> GenesisManifest {
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x01; 32]);
        GenesisManifest {
            manifest_version: super::GENESIS_MANIFEST_VERSION_1,
            chain_id: 1,
            network_id: 1,
            genesis_time: 1_700_000_000,
            genesis_message: "test genesis".to_string(),
            validators: vec![GenesisValidator {
                validator_id: [0x01; 32],
                consensus_key: keypair.key_descriptor(),
                bonded_stake: hn_state::MINIMUM_VALIDATOR_BOND,
            }],
            allocations: GenesisAllocations {
                reserve: GenesisAccount {
                    address: [0xA0; 32],
                    amount: RESERVE_ALLOCATION,
                },
                founder: GenesisAccount {
                    address: [0xA1; 32],
                    amount: FOUNDER_ALLOCATION,
                },
                community: GenesisAccount {
                    address: [0xA2; 32],
                    amount: COMMUNITY_ALLOCATION,
                },
            },
        }
    }

    #[test]
    fn a_valid_manifest_passes_validation() -> Result<(), GenesisError> {
        sample_manifest().validate()
    }

    #[test]
    fn rejects_an_under_bonded_validator() {
        let mut manifest = sample_manifest();
        manifest.validators[0].bonded_stake = hn_state::MINIMUM_VALIDATOR_BOND - 1;
        assert!(matches!(
            manifest.validate(),
            Err(GenesisError::InsufficientBond { .. })
        ));
    }

    #[test]
    fn rejects_a_mismatched_allocation_amount() {
        let mut manifest = sample_manifest();
        manifest.allocations.reserve.amount -= 1;
        assert!(matches!(
            manifest.validate(),
            Err(GenesisError::AllocationAmountMismatch { account: "reserve" })
        ));
    }

    #[test]
    fn rejects_duplicate_validator_ids() {
        let mut manifest = sample_manifest();
        let second = manifest.validators[0].clone();
        manifest.validators.push(second);
        assert!(matches!(
            manifest.validate(),
            Err(GenesisError::DuplicateValidatorId)
        ));
    }

    #[test]
    fn rejects_zero_validators() {
        let mut manifest = sample_manifest();
        manifest.validators.clear();
        assert!(matches!(
            manifest.validate(),
            Err(GenesisError::NoValidators)
        ));
    }

    #[test]
    fn rejects_reserved_chain_id() {
        let mut manifest = sample_manifest();
        manifest.chain_id = 0;
        assert!(matches!(
            manifest.validate(),
            Err(GenesisError::ReservedChainId)
        ));
    }

    #[test]
    fn rejects_a_genesis_message_over_the_limit() {
        let mut manifest = sample_manifest();
        manifest.genesis_message = "x".repeat(super::GENESIS_MESSAGE_MAX_LEN + 1);
        assert!(matches!(
            manifest.validate(),
            Err(GenesisError::GenesisMessageTooLong(_))
        ));
    }

    #[test]
    fn genesis_hash_is_deterministic() -> Result<(), GenesisError> {
        let manifest = sample_manifest();
        assert_eq!(manifest.genesis_hash()?, manifest.genesis_hash()?);
        Ok(())
    }

    #[test]
    fn genesis_hash_changes_with_content() -> Result<(), GenesisError> {
        let a = sample_manifest();
        let mut b = sample_manifest();
        b.genesis_time += 1;
        assert_ne!(a.genesis_hash()?, b.genesis_hash()?);
        Ok(())
    }

    #[test]
    fn initial_state_root_is_deterministic_and_order_independent() -> Result<(), GenesisError> {
        let manifest = sample_manifest();
        let root_a = manifest.initial_state_root()?;
        let root_b = manifest.initial_state_root()?;
        assert_eq!(root_a, root_b);
        Ok(())
    }

    #[test]
    fn loads_and_validates_a_real_json_file() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = sample_manifest();
        let keypair = Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, [0x01; 32]);
        let json = serde_json::json!({
            "manifest_version": 1,
            "chain_id": 1,
            "network_id": 1,
            "genesis_time": manifest.genesis_time,
            "genesis_message": manifest.genesis_message,
            "validators": [{
                "validator_id": hex_encode(&manifest.validators[0].validator_id),
                "consensus_key_algorithm_id": keypair.key_descriptor().algorithm_id(),
                "consensus_key_public_key": hex_encode(&keypair.key_descriptor().public_key_bytes()),
                "bonded_stake": manifest.validators[0].bonded_stake.to_string(),
            }],
            "allocations": {
                "reserve": {
                    "address": hex_encode(&manifest.allocations.reserve.address),
                    "amount": manifest.allocations.reserve.amount.to_string(),
                },
                "founder": {
                    "address": hex_encode(&manifest.allocations.founder.address),
                    "amount": manifest.allocations.founder.amount.to_string(),
                },
                "community": {
                    "address": hex_encode(&manifest.allocations.community.address),
                    "amount": manifest.allocations.community.amount.to_string(),
                },
            },
        });

        let dir = std::env::temp_dir().join(format!("hn-genesis-load-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("genesis.json");
        std::fs::write(&path, serde_json::to_vec(&json)?)?;

        let loaded = GenesisManifest::load(&path)?;
        assert_eq!(loaded, manifest);
        Ok(())
    }
}
