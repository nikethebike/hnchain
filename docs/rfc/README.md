# HNChain RFC

HNChain RFC documents define exact technical protocol specifications.

RFCs are implementation-facing and must include:

- normative behavior
- binary formats
- validation rules
- versioning rules
- compatibility rules
- security considerations
- test vectors when applicable

Whitepaper documents describe strategic direction. RFC documents define
implementable protocol behavior.

## RFC Families

- `core/`: shared primitive protocol types and cross-module rules
- `consensus/`: validator sets, leader selection, voting, finality, and safety
- `networking/`: P2P messages, transport negotiation, and peer behavior
- `storage/`: state access and storage engine boundaries
- `rpc/`: public node API behavior
- `wallet/`: wallet-facing interoperability
- `explorer/`: indexing and explorer-facing behavior
- `sdk/`: SDK compatibility rules
- `cli/`: command-line behavior
- `api/`: cross-interface API conventions
- `interoperability/`: external integration rules
