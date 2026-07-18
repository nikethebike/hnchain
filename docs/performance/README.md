# HNChain Performance

Status: Draft

Version: 0.1.0

Date: 2026-07-18

## Purpose

HNChain performance documentation defines measurement methodology, benchmark
profiles, metrics, reporting rules, and regression tracking.

Performance claims must be measurable and reproducible.

## Required Benchmark Context

Every published performance result must include:

- hardware profile
- operating system
- client version
- number of validators
- number of full nodes
- network latency model
- transaction size
- transaction type mix
- block size
- state size
- storage backend
- VM workload, if applicable
- test duration
- methodology

## Benchmark Profiles

Initial profile concepts:

```text
Small Network  -> 10 nodes
Medium Network -> 100 nodes
Large Network  -> 1000+ nodes
Stress Test    -> adversarial or saturation scenario
```

Exact profiles require separate RFCs.

## Metrics

HNChain should measure:

- throughput
- finality time
- block propagation delay
- CPU usage
- memory usage
- network bandwidth
- storage write latency
- storage read latency
- VM execution time
- RPC latency
- new-node sync time

TPS alone is not a sufficient performance report.
