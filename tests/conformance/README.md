# HNChain Conformance Tests

Status: Scaffold

Conformance tests verify language-independent protocol behavior against
accepted specifications and test vectors.

This directory is reserved for tests that independent implementations must be
able to reproduce.

## Core Vectors

- `core/primitive-types-v0.1.json` defines HNChain core primitive type
  boundaries.
- `core/hncs-primitives-v0.1.json` defines canonical serialization vectors for
  HNCS primitive values and is executed by the Rust `hn-hncs` test suite.
- `core/hncs-compound-v0.1.json` defines draft canonical serialization vectors
  for HNCS compound values. These vectors must be accepted before compound
  encoder and decoder implementations are added.
