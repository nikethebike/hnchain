/// Semantic protocol version used by HNChain protocol objects.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl ProtocolVersion {
    /// Creates a protocol version.
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Returns the major version component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the minor version component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// Returns the patch version component.
    #[must_use]
    pub const fn patch(self) -> u16 {
        self.patch
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolVersion;

    #[test]
    fn orders_lexicographically() {
        assert!(
            ProtocolVersion::new(1, 2, 0) > ProtocolVersion::new(1, 1, u16::MAX)
        );
        assert!(ProtocolVersion::new(2, 0, 0) > ProtocolVersion::new(1, u16::MAX, u16::MAX));
    }

    #[test]
    fn exposes_components() {
        let version = ProtocolVersion::new(1, 2, 3);

        assert_eq!(version.major(), 1);
        assert_eq!(version.minor(), 2);
        assert_eq!(version.patch(), 3);
    }
}
