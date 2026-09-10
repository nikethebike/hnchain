#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Cryptographic identity and primitive wrappers for HNChain.
//!
//! This crate must wrap reviewed external cryptographic implementations behind
//! HNChain-owned types. It must not define custom cryptographic algorithms.

mod hash;
mod identity;

pub use hash::{
    DIGEST_LEN, Digest, HASH_PROFILE_0X0001_ID, HashError, HashResult, hash_profile_0x0001,
};
pub use identity::{
    ED25519_ALGORITHM_ID, ED25519_PUBLIC_KEY_LEN, ED25519_SIGNATURE_LEN, Ed25519KeyPair,
    IdentityError, IdentityResult, KeyDescriptor, KeyRole,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_boundary_compiles() {}
}
