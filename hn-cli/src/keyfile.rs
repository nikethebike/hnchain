//! Key files (ADR-0047, "Decided: Key Files — Plain Seed File,
//! Owner-Only, Devnet/Testnet Grade"). Explicitly **not**
//! custody-grade: no passphrase encryption, no OS keychain, no
//! recovery phrase — adequate for devnet and testnet tokens only (see
//! that ADR's own "Rejected Options: Encrypted Keystore In v1").

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use hn_crypto::{Ed25519KeyPair, KeyRole, generate_seed};

/// The fixed first line of a key file.
pub const KEY_FILE_MAGIC: &str = "hnchain-key-v1";

/// The fixed second line — the only `key_role` this format writes
/// today (ADR-0047 names no other role for a CLI-managed key).
const ROLE_LINE: &str = "role: account-signing";

/// Why creating or loading a key file failed. No variant ever carries
/// the seed itself — this type exists specifically so a caller can
/// print `{error}` without risking the one thing ADR-0047 says must
/// never be printed.
#[derive(Debug)]
pub enum KeyFileError {
    /// `path` already exists; `generate` never overwrites one.
    AlreadyExists(PathBuf),
    /// A real I/O failure (permissions, missing directory, disk
    /// error).
    Io(std::io::Error),
    /// The file is readable by its group or by others (Unix only —
    /// never produced on a platform without Unix permissions).
    InsecurePermissions(PathBuf),
    /// The first three lines are not exactly [`KEY_FILE_MAGIC`], then
    /// [`ROLE_LINE`], then a `seed: ` line — includes a file with
    /// too few lines.
    MalformedHeader(String),
    /// The `seed: ` line's own value is not exactly 64 lowercase hex
    /// characters (uppercase is deliberately rejected, not merely
    /// non-hex — ADR-0047's own "Conformance / Test Requirements":
    /// "uppercase or non-hex seed").
    InvalidSeed,
}

impl fmt::Display for KeyFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(path) => {
                write!(
                    formatter,
                    "key file already exists, refusing to overwrite: {}",
                    path.display()
                )
            }
            Self::Io(error) => write!(formatter, "key file I/O error: {error}"),
            Self::InsecurePermissions(path) => write!(
                formatter,
                "key file is readable by group or others, refusing to load it: {} (fix: chmod 600 {})",
                path.display(),
                path.display()
            ),
            Self::MalformedHeader(reason) => write!(formatter, "malformed key file: {reason}"),
            Self::InvalidSeed => {
                write!(
                    formatter,
                    "key file's own seed is not exactly 64 lowercase hex characters"
                )
            }
        }
    }
}

impl std::error::Error for KeyFileError {}

impl From<std::io::Error> for KeyFileError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Generates a fresh seed from the operating system's own CSPRNG
/// ([`generate_seed`]) and writes it to `path` as a new key file —
/// `create_new` (never overwrites an existing file), mode `0600` on
/// Unix. Returns the resulting keypair; the caller decides what (if
/// anything) to print, but never has the raw seed bytes available to
/// print by mistake, since nothing here returns them.
pub fn generate(path: &Path) -> Result<Ed25519KeyPair, KeyFileError> {
    let seed = generate_seed();
    write_new(path, &seed)?;
    Ok(Ed25519KeyPair::from_seed(KeyRole::AccountSigning, seed))
}

/// Whether this platform's own filesystem permissions were actually
/// enforced by [`generate`]/[`load`] — `false` means the caller
/// should print ADR-0047's own warning that file protection could not
/// be enforced here.
#[must_use]
pub const fn enforces_unix_permissions() -> bool {
    cfg!(unix)
}

fn write_new(path: &Path, seed: &[u8; 32]) -> Result<(), KeyFileError> {
    let contents = format!(
        "{KEY_FILE_MAGIC}\n{ROLE_LINE}\nseed: {}\n",
        encode_lowercase_hex(seed)
    );

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            KeyFileError::AlreadyExists(path.to_path_buf())
        } else {
            KeyFileError::Io(error)
        }
    })?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

/// Loads an existing key file (ADR-0047's own "On load the CLI
/// *refuses* a key file readable by group or others").
pub fn load(path: &Path) -> Result<Ed25519KeyPair, KeyFileError> {
    check_permissions(path)?;
    let contents = fs::read_to_string(path)?;
    let mut lines = contents.lines();

    let magic = lines
        .next()
        .ok_or_else(|| KeyFileError::MalformedHeader("empty file".to_string()))?;
    if magic != KEY_FILE_MAGIC {
        return Err(KeyFileError::MalformedHeader(format!(
            "expected first line `{KEY_FILE_MAGIC}`, got `{magic}`"
        )));
    }

    let role = lines
        .next()
        .ok_or_else(|| KeyFileError::MalformedHeader("missing role line".to_string()))?;
    if role != ROLE_LINE {
        return Err(KeyFileError::MalformedHeader(format!(
            "expected second line `{ROLE_LINE}`, got `{role}`"
        )));
    }

    let seed_line = lines
        .next()
        .ok_or_else(|| KeyFileError::MalformedHeader("missing seed line".to_string()))?;
    let seed_hex = seed_line.strip_prefix("seed: ").ok_or_else(|| {
        KeyFileError::MalformedHeader(format!("malformed seed line: {seed_line}"))
    })?;
    let seed = decode_lowercase_hex_32(seed_hex)?;

    Ok(Ed25519KeyPair::from_seed(KeyRole::AccountSigning, seed))
}

#[cfg(unix)]
fn check_permissions(path: &Path) -> Result<(), KeyFileError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)?;
    let mode = metadata.permissions().mode();
    // Any bit for "group" or "other" (the low 6 bits, 0o077) means
    // someone besides the owner can read (or write/execute) this
    // file -- refused regardless of which specific bit it is.
    if mode & 0o077 != 0 {
        return Err(KeyFileError::InsecurePermissions(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path) -> Result<(), KeyFileError> {
    Ok(())
}

fn encode_lowercase_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_lowercase_hex_32(text: &str) -> Result<[u8; 32], KeyFileError> {
    if text.len() != 64 {
        return Err(KeyFileError::InvalidSeed);
    }
    let mut seed = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = lowercase_hex_nibble(pair[0]).ok_or(KeyFileError::InvalidSeed)?;
        let low = lowercase_hex_nibble(pair[1]).ok_or(KeyFileError::InvalidSeed)?;
        seed[index] = (high << 4) | low;
    }
    Ok(seed)
}

/// Deliberately accepts only `0-9`/`a-f` — uppercase is a real
/// rejection case this format cares about (see [`KeyFileError::InvalidSeed`]'s
/// own doc comment), not merely "any hex digit" the way a lenient
/// decoder elsewhere in this workspace (`hn-rpc`'s own request-path
/// hex) treats input.
const fn lowercase_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyFileError, generate, load};

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "hnchain-keyfile-test-{name}-{}",
            std::process::id()
        ))
    }

    /// Writes a raw fixture file for a test that is not itself about
    /// file permissions -- `std::fs::write` alone leaves the file at
    /// whatever the process's own umask grants (often group/other
    /// readable on a real Unix machine, e.g. CI's own Linux runner,
    /// even though never on Windows, which has no such concept at
    /// all), which `load`'s own `#[cfg(unix)]` permission check would
    /// then reject before ever reaching the header/seed parsing these
    /// tests actually exercise.
    fn write_fixture_with_secure_permissions(
        path: &std::path::Path,
        contents: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(path, contents)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    #[test]
    fn generate_then_load_reproduces_the_same_keypair() -> Result<(), Box<dyn std::error::Error>> {
        let path = temp_path("round-trip");
        let _ = std::fs::remove_file(&path);

        let generated = generate(&path)?;
        let loaded = load(&path)?;
        assert_eq!(
            generated.key_descriptor().public_key_bytes(),
            loaded.key_descriptor().public_key_bytes()
        );

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn generate_refuses_to_overwrite_an_existing_file() -> Result<(), Box<dyn std::error::Error>> {
        let path = temp_path("no-overwrite");
        let _ = std::fs::remove_file(&path);
        generate(&path)?;

        let result = generate(&path);
        assert!(matches!(result, Err(KeyFileError::AlreadyExists(_))));

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn generate_sets_mode_0600() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("mode-0600");
        let _ = std::fs::remove_file(&path);
        generate(&path)?;

        let mode = std::fs::metadata(&path)?.permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn load_refuses_a_group_or_other_readable_file() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("insecure-perms");
        let _ = std::fs::remove_file(&path);
        generate(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;

        let result = load(&path);
        assert!(matches!(result, Err(KeyFileError::InsecurePermissions(_))));

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn load_rejects_a_malformed_header() -> Result<(), Box<dyn std::error::Error>> {
        let path = temp_path("malformed-header");
        let _ = std::fs::remove_file(&path);
        write_fixture_with_secure_permissions(&path, "not-the-right-magic\n")?;

        let result = load(&path);
        assert!(matches!(result, Err(KeyFileError::MalformedHeader(_))));

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn load_rejects_a_wrong_length_seed() -> Result<(), Box<dyn std::error::Error>> {
        let path = temp_path("wrong-length-seed");
        let _ = std::fs::remove_file(&path);
        write_fixture_with_secure_permissions(
            &path,
            "hnchain-key-v1\nrole: account-signing\nseed: aabb\n",
        )?;

        let result = load(&path);
        assert!(matches!(result, Err(KeyFileError::InvalidSeed)));

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn load_rejects_an_uppercase_seed() -> Result<(), Box<dyn std::error::Error>> {
        let path = temp_path("uppercase-seed");
        let _ = std::fs::remove_file(&path);
        let uppercase_seed = "AB".repeat(32);
        write_fixture_with_secure_permissions(
            &path,
            &format!("hnchain-key-v1\nrole: account-signing\nseed: {uppercase_seed}\n"),
        )?;

        let result = load(&path);
        assert!(matches!(result, Err(KeyFileError::InvalidSeed)));

        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn load_rejects_non_hex_characters() -> Result<(), Box<dyn std::error::Error>> {
        let path = temp_path("non-hex-seed");
        let _ = std::fs::remove_file(&path);
        let bad_seed = "z".repeat(64);
        write_fixture_with_secure_permissions(
            &path,
            &format!("hnchain-key-v1\nrole: account-signing\nseed: {bad_seed}\n"),
        )?;

        let result = load(&path);
        assert!(matches!(result, Err(KeyFileError::InvalidSeed)));

        std::fs::remove_file(&path)?;
        Ok(())
    }
}
