//! ADR-0048's own required proof, named explicitly in "Decided:
//! Certificate Routing In `hn-node`" item 5: "a late node receives a
//! precommit certificate for an *older round* of the current height,
//! plus the block, and finalizes. Without this test an ADR-0048
//! implementation can look complete while R8 and R10 never run."
//!
//! A real `hn-node` validator (index 3 of 4) is started peered with
//! only a single scripted peer -- never the other three real
//! validators -- so it can only ever learn about height 1 from
//! messages this test hand-builds and sends itself, never from a real
//! quorum of its own peers or from ADR-0044's historical sync (there
//! is nothing to sync against: the scripted peer never answers a
//! `SyncBlockRangeRequest`). Left alone, it cycles nil `prevote`/
//! `precommit`/round-advance through its own propose/prevote/precommit
//! timeouts forever, advancing its own round with nothing to vote for
//! -- exactly the state a real validator would be in after genuinely
//! missing a round's own proposal.
//!
//! The scripted peer waits long enough for that local round to have
//! already advanced past 0 at least once, then delivers, in order:
//! (1) a real, validly-signed `precommit` quorum certificate for
//! height 1, round 0 (a round the real node's own local round has by
//! then already left behind) targeting a block it has never seen --
//! this must produce `ConsensusAction::NeedBlock` (observable as this
//! process's own `WAITING_FOR_BLOCK` log line, not a crash, not a
//! silently dropped message the old `expects_qc`/`is_current` filter
//! would have produced); (2) the real, validly-signed proposal for
//! that exact block -- `ConsensusEngine::block_fetched`'s retry must
//! then finalize it.
//!
//! Three of the genesis file's four validators (0, 1, 2) sign the
//! certificate -- 3 of 4 equal shares is a real `2f+1` quorum (`f = 1`)
//! -- and validator 1 is height 1/round 0's own real
//! `round_proposer`, so its signature over the block is the real
//! proposer authentication a receiver actually checks, not a
//! shortcut.

mod support;

use std::time::Duration;

use hn_core::{BlockHeight, Epoch, ProtocolEpoch, Round, UnixTimeMillis};
use hn_crypto::{Digest, Ed25519KeyPair, KeyRole, SignatureEnvelope};
use hn_network::ConsensusProposalMessageV1;
use hn_node::GenesisManifest;
use hn_state::{
    BLOCK_VERSION_1, BODY_VERSION_1, BlockBody, BlockEnvelope, BlockHeader, HEADER_VERSION_1,
    QuorumCertificate, VoteSigningPayloadV1, VoteTargetType, VoteType, extra_data_hash,
    list_empty_root, protocol_parameters_placeholder_hash,
};

use support::{
    TestResult, consensus_key_seed, listen_addr, spawn_node, spawn_precommit_then_block_peer,
    wait_until_all_log, write_devnet_genesis,
};

const VALIDATOR_COUNT: u8 = 4;
const BASE_PORT: u16 = 32_900;
const BASE_TIMEOUT_MS: u64 = 120;
const WAIT: Duration = Duration::from_secs(60);
const LATE_VALIDATOR: u8 = 3;
const SCRIPTED_PEER_INDEX: u8 = 4;
// `[0, 1, 2]` of 4 equal shares is 75%, comfortably above `2f+1` (67%)
// for `f = 1`.
const SIGNING_VALIDATORS: [u8; 3] = [0, 1, 2];
const PROPOSER_INDEX: u8 = 1;
// A fixed devnet validator_set_commitment -- matches `hn-node`'s own
// `DEVNET_VSC` by value (not importable: not `pub`, same reasoning
// every other devnet fixture in `support` already duplicates rather
// than reaching into the real binary's own internals for).
const DEVNET_VSC: Digest = [0x11; 32];

#[test]
fn a_late_node_finalizes_an_older_round_precommit_once_the_block_arrives() -> TestResult {
    let data_root = std::env::temp_dir().join(format!(
        "hn-node-precommit-before-proposal-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_root)?;

    let genesis_path = data_root.join("genesis.json");
    write_devnet_genesis(&genesis_path, VALIDATOR_COUNT)?;
    let manifest = GenesisManifest::load(&genesis_path)?;

    let genesis_block_hash = genesis_block_hash(&manifest)?;
    let block_hash = height_one_block_hash(&manifest, genesis_block_hash)?;
    let qc_payload =
        height_one_precommit_qc(manifest.chain_id, manifest.network_id, block_hash)?.encode()?;
    let proposal_payload = height_one_proposal(&manifest, genesis_block_hash)?.encode()?;

    let scripted_peer_addr = listen_addr(BASE_PORT, SCRIPTED_PEER_INDEX);
    // Long enough for the real node's own propose/prevote/precommit
    // timeouts to cycle it through at least one full round advance
    // before anything arrives -- proving the certificate below really
    // targets a round this node's own local state has already left
    // behind, not merely the round it happened to start at.
    let delay_before_block = Duration::from_millis(BASE_TIMEOUT_MS * 6);
    spawn_precommit_then_block_peer(
        scripted_peer_addr,
        manifest.chain_id,
        manifest.network_id,
        qc_payload,
        delay_before_block,
        proposal_payload,
    )?;

    let data_dir = data_root.join("late-validator");
    std::fs::create_dir_all(&data_dir)?;
    let late_node = spawn_node(
        &genesis_path,
        &data_dir,
        LATE_VALIDATOR,
        BASE_PORT,
        &[LATE_VALIDATOR, SCRIPTED_PEER_INDEX],
        BASE_TIMEOUT_MS,
    )?;

    // Proves ConsensusAction::NeedBlock really fired -- the old
    // expects_qc/is_current caller-side filter would have silently
    // dropped this certificate instead of ever reaching the engine.
    wait_until_all_log(
        &[&late_node],
        |line| line.starts_with("WAITING_FOR_BLOCK height=1 "),
        WAIT,
    )?;

    let finalized_lines = wait_until_all_log(
        &[&late_node],
        |line| line.starts_with("FINALIZED height=1 "),
        WAIT,
    )?;
    if !finalized_lines[0].contains(&support::hex(&block_hash)) {
        return Err(format!(
            "finalized a different block than the one certified: {}",
            finalized_lines[0]
        )
        .into());
    }

    Ok(())
}

fn validator_keypair(index: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed(KeyRole::ValidatorConsensus, consensus_key_seed(index))
}

fn empty_body(extra_data: Vec<u8>) -> BlockBody {
    BlockBody {
        body_version: BODY_VERSION_1,
        transactions: Vec::new(),
        receipts: Vec::new(),
        evidence: Vec::new(),
        extra_data,
    }
}

/// Mirrors `hn-node`'s own private `genesis_block_header` exactly (not
/// importable: not `pub`) -- every real validator peered with this
/// scripted one independently computes the identical hash from the
/// identical genesis file, so this duplication must stay byte-for-byte
/// faithful, not just "close enough."
fn genesis_block_hash(manifest: &GenesisManifest) -> TestResult<Digest> {
    let body = empty_body(manifest.extra_data.clone());
    let header = BlockHeader {
        header_version: HEADER_VERSION_1,
        chain_id: manifest.chain_id,
        network_id: manifest.network_id,
        height: BlockHeight::GENESIS,
        round: Round::new(0),
        epoch: Epoch::new(0),
        protocol_epoch: ProtocolEpoch::GENESIS,
        parent_block_hash: [0_u8; 32],
        proposer: [0_u8; 32],
        timestamp: UnixTimeMillis::from_millis(manifest.genesis_time.saturating_mul(1000)),
        transactions_root: body.transactions_root()?,
        state_root: manifest.initial_state_root()?,
        receipts_root: body.receipts_root()?,
        events_root: list_empty_root()?,
        consensus_root: DEVNET_VSC,
        evidence_root: body.evidence_root()?,
        protocol_parameters_hash: protocol_parameters_placeholder_hash()?,
        extra_data_hash: extra_data_hash(&manifest.extra_data)?,
    };
    Ok(header.block_hash()?)
}

/// A real, validly-signed, zero-transaction height-1/round-0 block.
/// Zero transactions change nothing from genesis's own already-
/// committed state, so its own `state_root` is exactly
/// `manifest.initial_state_root()` -- the same post-state-root
/// machinery a real block-processing pipeline would produce applying
/// an empty write-set on top of that identical base state, not a
/// separate, only-coincidentally-equal value.
fn height_one_envelope(
    manifest: &GenesisManifest,
    genesis_block_hash: Digest,
) -> TestResult<BlockEnvelope> {
    let body = empty_body(Vec::new());
    let header = BlockHeader {
        header_version: HEADER_VERSION_1,
        chain_id: manifest.chain_id,
        network_id: manifest.network_id,
        height: BlockHeight::new(1),
        round: Round::new(0),
        epoch: Epoch::new(0),
        protocol_epoch: ProtocolEpoch::GENESIS,
        parent_block_hash: genesis_block_hash,
        proposer: [PROPOSER_INDEX; 32],
        timestamp: UnixTimeMillis::from_millis(manifest.genesis_time.saturating_mul(1000) + 1),
        transactions_root: body.transactions_root()?,
        state_root: manifest.initial_state_root()?,
        receipts_root: body.receipts_root()?,
        events_root: list_empty_root()?,
        consensus_root: DEVNET_VSC,
        evidence_root: body.evidence_root()?,
        protocol_parameters_hash: protocol_parameters_placeholder_hash()?,
        extra_data_hash: extra_data_hash(&[])?,
    };
    Ok(BlockEnvelope {
        block_version: BLOCK_VERSION_1,
        header,
        body,
        justification: None,
    })
}

fn height_one_block_hash(
    manifest: &GenesisManifest,
    genesis_block_hash: Digest,
) -> TestResult<Digest> {
    Ok(height_one_envelope(manifest, genesis_block_hash)?.block_hash()?)
}

fn height_one_proposal(
    manifest: &GenesisManifest,
    genesis_block_hash: Digest,
) -> TestResult<ConsensusProposalMessageV1> {
    let envelope = height_one_envelope(manifest, genesis_block_hash)?;
    let block_hash = envelope.block_hash()?;
    let proposer_keypair = validator_keypair(PROPOSER_INDEX);
    let proposer_signature = SignatureEnvelope {
        algorithm_id: proposer_keypair.key_descriptor().algorithm_id(),
        key_reference: None,
        signature: proposer_keypair.sign(&block_hash).to_vec(),
    };
    Ok(ConsensusProposalMessageV1 {
        envelope,
        proposer_signature,
    })
}

/// A real `2f+1` precommit quorum certificate for `block_hash` at
/// height 1, round 0, signed by three of the genesis file's four
/// validators (indices 0, 1, 2) -- each devnet validator holds equal
/// `bonded_stake`/`voting_power`, so 3 of 4 is 75% of total voting
/// power, above the `signed * 3 > total * 2` threshold.
fn height_one_precommit_qc(
    chain_id: u8,
    network_id: u16,
    block_hash: Digest,
) -> TestResult<QuorumCertificate> {
    let total_voting_power: u128 =
        u128::from(VALIDATOR_COUNT) * hn_state::MINIMUM_VALIDATOR_BOND * 10;
    let signed_voting_power: u128 =
        u128::try_from(SIGNING_VALIDATORS.len())? * hn_state::MINIMUM_VALIDATOR_BOND * 10;

    let mut aggregate_proof = Vec::new();
    let mut signer_commitment = 0_u8;
    for &index in &SIGNING_VALIDATORS {
        let keypair = validator_keypair(index);
        let payload = VoteSigningPayloadV1 {
            vote_type: VoteType::Precommit,
            chain_id,
            network_id,
            epoch: Epoch::new(0),
            height: BlockHeight::new(1),
            round: Round::new(0),
            validator_set_commitment: DEVNET_VSC,
            validator_id: [index; 32],
            target_type: VoteTargetType::Block,
            target_hash: block_hash,
            vote_metadata: Vec::new(),
        };
        let digest = payload.signing_digest()?;
        aggregate_proof.push(SignatureEnvelope {
            algorithm_id: keypair.key_descriptor().algorithm_id(),
            key_reference: None,
            signature: keypair.sign(&digest).to_vec(),
        });
        signer_commitment |= 1 << index;
    }

    Ok(QuorumCertificate {
        certificate_type: VoteType::Precommit,
        chain_id,
        network_id,
        epoch: Epoch::new(0),
        height: BlockHeight::new(1),
        round: Round::new(0),
        validator_set_commitment: DEVNET_VSC,
        target_type: VoteTargetType::Block,
        target_hash: block_hash,
        total_voting_power,
        signed_voting_power,
        signer_commitment: vec![signer_commitment],
        aggregate_proof,
    })
}
