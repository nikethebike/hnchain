use std::fmt;

use hn_consensus::ConsensusError;
use hn_network::NetworkError;
use hn_state::StateError;

use crate::config::ConfigError;

/// Errors this crate's node process orchestration can produce
/// (ADR-0037, "Multi-Node Consensus Wiring").
#[derive(Debug)]
pub enum NodeError {
    /// `NodeConfig::parse` rejected the process's own arguments.
    Config(ConfigError),
    /// A real I/O operation (binding a listener, opening the state
    /// store's file) failed.
    Io(std::io::Error),
    /// A `hn-state` operation failed.
    State(StateError),
    /// A `hn-consensus` state-machine/engine operation failed.
    Consensus(ConsensusError),
    /// A `hn-network` protocol operation failed.
    Network(NetworkError),
}

impl fmt::Display for NodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::State(error) => write!(formatter, "state error: {error}"),
            Self::Consensus(error) => write!(formatter, "consensus error: {error}"),
            Self::Network(error) => write!(formatter, "network error: {error}"),
        }
    }
}

impl std::error::Error for NodeError {}

impl From<ConfigError> for NodeError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<std::io::Error> for NodeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StateError> for NodeError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

impl From<ConsensusError> for NodeError {
    fn from(error: ConsensusError) -> Self {
        Self::Consensus(error)
    }
}

impl From<NetworkError> for NodeError {
    fn from(error: NetworkError) -> Self {
        Self::Network(error)
    }
}

/// Shorthand for `Result<T, NodeError>`.
pub type NodeResult<T> = Result<T, NodeError>;
