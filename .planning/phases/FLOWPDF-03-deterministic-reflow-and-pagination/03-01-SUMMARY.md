---
phase: FLOWPDF-03-deterministic-reflow-and-pagination
plan: "01"
subsystem: deterministic-text-layout
tags: [layout, fixed-point, shaping, unicode, hyphenation, provenance]
requires:
  - phase: FLOWPDF-02-accessible-rich-text-editing
    provides: Canonical FlowDocument text, transaction revisions, and semantic editor ownership
provides:
  - Rust-owned fixed-point text layout request/result contract
  - Explicit byte-backed font catalog with deterministic identity and fail-closed unsupported-glyph diagnostics
  - ICU grapheme/line segmentation, Unicode bidi, RustyBuzz shaping, and versioned Ukrainian hyphenation seams
affects: [phase-3-pagination, phase-3-workers, phase-3-rendering]
tech-stack:
  added: [hyphenation 0.8.4, rustybuzz 0.20.1, ttf-parser 0.25.1, unicode-bidi 0.3.18]
  patterns:
    - Keep canonical text separate from derived glyph/line fragments and bind results to source revision/hash and engine identities.
    - Use checked 1/64-point integer geometry and explicit admitted font/data bytes; never discover host fonts.
    - Preserve UTF-8/UTF-16 source ranges and reject unsafe shaped-cluster breaks without inserting soft hyphens into stored text.
key-files:
  created:
    - crates/flow-core/src/layout/mod.rs
    - crates/flow-core/tests/layout_fixed_point.rs
    - crates/flow-core/tests/layout_text.rs
    - crates/flow-core/data/NotoSans-Regular.ttf
    - crates/flow-core/data/NotoSans-OFL.txt
    - crates/flow-core/data/uk.standard.bincode
    - fixtures/layout/font-provenance.json
    - fixtures/layout/phase3-text-corpus.json
  modified:
    - Cargo.toml
    - Cargo.lock
    - crates/flow-core/Cargo.toml
    - crates/flow-core/src/lib.rs
    - .planning/phases/FLOWPDF-03-deterministic-reflow-and-pagination/03-01-PLAN.md
key-decisions:
  - The first admitted fixture font is the checked-in Noto Sans Regular binary with its OFL text and SHA-256 provenance; no system-font fallback is available.
  - Ukrainian hyphenation is admitted as one checked-in Standard-pattern bincode artifact with identity/hash validation; missing or mismatched data fails closed.
  - The tracer is intentionally paragraph-scoped: pagination, page geometry, schema-v3 sections, workers, and PDF output remain later plans.
  - The current implementation covers the admitted fixture and explicit unsupported-data paths; it does not claim complete production coverage for every script or font feature.
requirements-completed: [LAYO-01, LAYO-02]
coverage:
  - id: T1
    description: Fixed-point request geometry and bounded font catalog identities are deterministic and checked.
    requirement: LAYO-01
    verification:
      - kind: test
        ref: crates/flow-core/tests/layout_fixed_point.rs
        status: pass
        note: 3 focused integration tests passed, including checked conversions, overflow, malformed font, and duplicate identity cases.
  - id: T2
    description: Supported text is segmented, shaped, direction-mapped, and line-broken through explicit data adapters while preserving source ranges.
    requirement: LAYO-02
    verification:
      - kind: test
        ref: crates/flow-core/tests/layout_text.rs
        status: pass
        note: 5 focused integration tests passed for Ukrainian/English text, decomposed Unicode, emoji, UTF-16 clusters, forced direction, hyphenation, hash stability, and fail-closed data/catalog paths.
  - id: T3
    description: The new layout module does not regress the existing Rust or WASM crates.
    requirement: all
    verification:
      - kind: regression
        ref: cargo test --locked -p flow-core and cargo test --locked -p flow-wasm
        status: pass
        note: All flow-core suites and the flow-wasm test targets passed.
verification:
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo tree --locked -p flow-core -e features
    result: pass
    note: Only the pinned layout dependencies and intended features are admitted; no hyphenation embed_all/all-language feature or host-font dependency was added.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_fixed_point --test layout_text -- --nocapture
    result: pass
    note: 8 focused layout integration tests passed.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet
    result: pass
    note: Complete flow-core unit and integration suite passed, including the new layout suites and the preserved grapheme conformance work.
  - command: env RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked -p flow-core --all-targets -- -D warnings
    result: pass
    note: No warnings in the new or existing flow-core targets.
  - command: npm run build:wasm
    result: pass
    note: Rust release build and wasm-bindgen generation completed with the pinned toolchain.
  - command: npm run typecheck
    result: pass
    note: TypeScript shell remains type-safe after exposing the layout module in the workspace.
  - command: git diff --check
    result: pass
duration: same session
completed: 2026-09-21
status: complete
---

# Phase 3 Plan 01: Deterministic Text Layout Summary

Plan 03-01 is complete. FlowPDF now has the first Rust-owned Phase 3 vertical
tracer: fixed-point paragraph shaping and line opportunities bound to explicit
font/data identities. The implementation is deliberately below pagination and
does not claim a finished editor layout engine or PDF renderer.

## Accomplishments

- Added a checked `LayoutUnit` based on 1/64 point geometry and bounded rational
  conversion from millipoints and font units.
- Added revision-bound `TextLayoutRequest` and deterministic result hashing.
- Added a byte-backed `FontCatalog` with Noto Sans Regular as a checked-in,
  licensed fixture; malformed, duplicate, oversized, or unsupported inputs fail
  with stable diagnostics and never consult system fonts.
- Added ICU grapheme and line segmentation, Unicode bidi runs, RustyBuzz glyph
  shaping, UTF-8/UTF-16 source cluster mapping, unsafe-break protection, and
  checked fixed-point advances.
- Added versioned Ukrainian hyphenation data and explicit provenance/hash
  fixtures. Discretionary breaks remain source metadata; canonical text is not
  mutated.
- Added focused corpus and integration tests for fixed-point boundaries,
  direction, clusters, fallback/error paths, hyphenation, and stable hashes.

## Verification

- `cargo test --locked -p flow-core --quiet` — pass.
- `cargo test --locked -p flow-wasm --quiet` — pass.
- `cargo clippy --locked -p flow-core --all-targets -- -D warnings` — pass.
- `npm run build:wasm` and `npm run typecheck` — pass.
- Dependency feature inspection and `git diff --check` — pass.

## Next Phase 3 Step

Plan 03-02 will extend the canonical model and migration boundary for durable
section/header/footer settings. Pagination and worker integration remain later
plans; this tracer is their deterministic text-layout input, not their final
page representation.

## Self-check: PASSED

The implementation is scoped to the admitted font/data fixture and explicit
diagnostics. It does not claim full Unicode shaping coverage, pagination, PDF
output, or Phase 3 completion.
