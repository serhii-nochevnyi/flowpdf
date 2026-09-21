---
phase: FLOWPDF-03-deterministic-reflow-and-pagination
plan: "05"
subsystem: wasm-layout-worker
tags: [wasm, layout-protocol, worker, stale-results, cancellation, revision-safety]
requires:
  - phase: FLOWPDF-03-deterministic-reflow-and-pagination
    provides: Revision/hash-bound paginator, privacy-safe layout diagnostics, and deterministic result hashes
provides:
  - Closed versioned serialized Rust/WASM layout request and response DTOs
  - Bounded explicit font/data ingress with no raw bytes or authored text in results
  - Rust result-hash verification and immutable worker publication boundary
  - Revision-aware TypeScript scheduler with abort, stale-result, identity, and failure recovery guards
  - Editor controller bridge that schedules derived layout without blocking semantic input/session work
affects: [phase-3-viewport, phase-3-gate]
tech-stack:
  added: []
  patterns:
    - Use a string-only WASM layout request/response API; Rust validates canonical bytes, identities, bounds, and result hashes.
    - Keep worker layout as a single-flight derived projection; abort superseded work and publish only a complete accepted result.
    - Compare request ID, source revision/hash, settings, font/data identities, and Rust-verified result hash before publication.
key-files:
  created:
    - crates/flow-wasm/tests/layout_exports.rs
    - web/src/layout/layout-protocol.ts
    - web/src/layout/layout-worker.ts
    - web/tests/layout-worker.test.ts
  modified:
    - crates/flow-core/src/layout/mod.rs
    - crates/flow-core/src/lib.rs
    - crates/flow-wasm/Cargo.toml
    - crates/flow-wasm/src/lib.rs
    - web/src/editor/editor-controller.ts
    - Cargo.lock
requirements-completed: [LAYO-08]
coverage:
  - id: T1
    description: The serialized Rust/WASM boundary accepts only bounded closed DTOs and returns derived pages, source ranges, identities, diagnostics, and hashes.
    requirement: LAYO-08
    verification:
      - kind: test
        ref: crates/flow-wasm/tests/layout_exports.rs
        status: pass
        note: Determinism, unknown/future DTO rejection, source/identity mismatch, request size limits, no authored-text/font-field leakage, and response hash tampering are covered.
  - id: T2
    description: Out-of-order and stale worker results cannot replace a newer accepted layout while semantic editing remains independent.
    requirement: LAYO-08
    verification:
      - kind: test
        ref: web/tests/layout-worker.test.ts
        status: pass
        note: Newest-result publication, request/source/settings/font/data guards, result hash rejection, cooperative cancellation, failure recovery, and the string-only WASM adapter are covered.
  - id: T3
    description: The editor controller exposes layout separately and starts background work after accepted document publications without extending command/session barriers.
    requirement: LAYO-08
    verification:
      - kind: typecheck
        ref: web/src/editor/editor-controller.ts
        status: pass
        note: Optional scheduler/factory bridge, layout snapshot/subscription, request guard, and disposal cancellation compile with the existing controller contracts.
verification:
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo fmt --all -- --check
    result: pass
    note: Rust formatting is clean.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --offline -p flow-wasm --test layout_exports -- --nocapture
    result: pass
    note: Four closed-boundary tests passed.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --offline --locked --workspace --all-targets -- -D warnings
    result: pass
    note: Core, WASM boundary, and export tests pass with no warnings.
  - command: npm run build:wasm
    result: pass
    note: Release flow-wasm build and wasm-bindgen generation completed.
  - command: npm run typecheck
    result: pass
    note: Protocol, worker, and controller bridge are type-safe.
  - command: npx --no-install vitest run web/tests/layout-worker.test.ts
    result: pass
    note: Four worker/scheduler tests passed.
  - command: git diff --check
    result: pass
    note: No whitespace errors.
duration: same session
completed: 2026-09-21
status: complete
---

# Phase 3 Plan 05: WASM Layout Boundary and Worker Scheduling Summary

Plan 03-05 is complete. FlowPDF now has a narrow string-only layout boundary
and a revision-aware worker scheduler. Rust admits canonical document bytes,
explicit bounded font/data bytes, identities, and viewport limits, then returns
only derived page fragments, source ranges, allowlisted diagnostics, and a
deterministic result hash. The result verifier recomputes that hash before a
worker can publish it.

## Accomplishments

- Added versioned `LayoutWasmRequest`/`LayoutWasmResponse` DTOs with denied
  unknown fields, future-version rejection, aggregate request/result budgets,
  viewport bounds, canonical source checks, font/data identity checks, and
  stable privacy-safe error codes.
- Added `layout_document` and `verify_layout_response` WASM exports. They
  exchange JSON strings and never expose mutable Rust layout handles, system
  font state, authored diagnostic payloads, or raw font bytes in results.
- Added a worker protocol and `RevisionAwareLayoutScheduler` that aborts
  superseded work, rejects out-of-order/stale identity mismatches, verifies
  complete results, and publishes one immutable projection atomically.
- Added a controller bridge for optional background layout scheduling, layout
  snapshot/subscription access, source-revision guards, and disposal cleanup;
  semantic command and editor-session barriers remain independent.

## Scope boundary

The worker scheduler transports and accepts complete Rust-derived results; it
does not yet render page fragments or virtualize the viewport. Page projection,
semantic/visual revision synchronization, and the repeatable Phase 3 gate are
the remaining 03-06 scope.

## Self-check: PASSED

Rust/WASM boundary tests, Clippy, release WASM generation, TypeScript
typecheck, worker tests, and formatting/diff checks all pass. Stale or partial
results cannot replace the accepted layout projection.
