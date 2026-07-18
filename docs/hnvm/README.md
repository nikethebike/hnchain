# HNVM Specification

HNVM documents define the HNChain virtual machine and smart contract execution
model.

HNVM specifications must define:

- deterministic execution rules
- instruction set or target format
- resource metering
- memory model
- host functions
- account and storage access
- event and receipt behavior
- failure semantics
- upgrade rules
- security constraints

HNVM must not expose nondeterministic host behavior to consensus execution.
