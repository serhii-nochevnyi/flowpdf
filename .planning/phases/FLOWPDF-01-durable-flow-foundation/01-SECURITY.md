---
phase: 1
slug: durable-flow-foundation
status: verified
threats_open: 0
threats_total: 25
asvs_level: 1
blocking_threshold: high
register_authored_at_plan_time: true
created: 2026-08-25
updated: 2026-08-25
---

# Phase 1 — Security

> ASVS L1 verification of the plan-authored STRIDE register. Every high-severity mitigation is present and exercised; accepted lower-severity risks are documented explicitly.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Internet registries and repositories → workspace | Package metadata, archives, installers, and repository identity are hostile until verified. | Executable supply-chain inputs |
| Lockfiles → executable build | Origin, checksum, integrity, or direct-version drift can redirect installation. | Dependency graph and artifacts |
| Browser/TypeScript → Rust/WASM | Commands, identifiers, revisions, stored records, and error inputs are untrusted. | Semantic mutation DTOs |
| IndexedDB/native records → Rust recovery | Physical records can be partial, corrupt, conflicting, replayed, or oversized. | Documents, transactions, assets, audit records |
| Persisted JSON and asset bytes → typed model | Bytes can be malformed, future-versioned, substituted, or resource-exhausting. | Semantic document and binary assets |
| Input modality/DOM → command service | Browser events, offsets, targets, and displayed revisions can be stale or manipulated. | Typed command envelopes |
| Stored history → undo/redo | Altered inverses or mismatched history can target different content. | Forward/inverse operations and anchors |
| Command/transaction data → audit projection | Recovery-required semantic content must not become a shadow content store. | Privacy-minimized audit metadata |
| Audit/provenance DTO → DOM/accessibility tree | Rendering can disclose content or imply unsupported lineage. | User-visible diagnostics and provenance |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation and evidence | Status |
|-----------|----------|-----------|----------|-------------|-------------------------|--------|
| T-01-SC | Tampering | Cargo/npm/rustup/browser installs | high | mitigate | Official-source allowlist, origin/integrity/checksum verifier, pinned locks, and passing provenance artifact/tests. | closed |
| T-01-02 | Spoofing | Rust installer/toolchain | high | mitigate | Official-host receipt and binary checksum verification runs before execution; exact Rust and wasm-bindgen versions are pinned. | closed |
| T-01-03 | Denial of Service | Registry/repository availability | medium | accept | Installation fails closed with bounded retries and a blocker rather than publishing partial trust. See AR-01. | closed |
| T-01-04 | Information Disclosure | Provenance diagnostics | low | accept | Reports contain public package metadata and tool versions only; no credentials or environment dumps. See AR-02. | closed |
| T-02-01 | Tampering | WASM command boundary | high | mitigate | Strict DTO decoding, revision checks, immutable Rust application, stable errors, and no mutable semantic handle in JavaScript; boundary tests pass. | closed |
| T-02-02 | Tampering / Denial of Service | IndexedDB commit/reload | high | mitigate | Related records commit atomically, acknowledgement follows completion, and Rust revalidates revision/hash after reload; browser interruption/recovery tests pass. | closed |
| T-02-03 | Information Disclosure | Inspector audit/provenance | high | mitigate | Rust-owned closed audit DTO plus text-only allowlisted DOM projection; negative redaction tests pass. | closed |
| T-02-04 | Elevation of Privilege | Local inspector controls | low | accept | Phase 1 has no identity, backend, authorization, or privileged operation; controls use the local typed command boundary. See AR-03. | closed |
| T-03-01 | Tampering | Canonical decode/hash | high | mitigate | Typed invariants, cross-reference checks, exact re-encoding, versioned hash, and no publication before validation; schema/persistence tests pass. | closed |
| T-03-02 | Denial of Service | JSON/model decode | high | mitigate | Byte/depth/node/text/asset/field ceilings are enforced before growth and tested at N and N+1. | closed |
| T-03-03 | Tampering | Migration registry | high | mitigate | Only contiguous pure hops are permitted; every boundary is validated and golden-replayed twice. | closed |
| T-03-04 | Spoofing | BLAKE3 interpretation | low | accept | Product copy and code treat unkeyed hashes as equality/integrity evidence, never authentication. See AR-04. | closed |
| T-04-01 | Tampering | Command target/revision | high | mitigate | Revision, identity, range, UTF-16 boundary, invariant, and anchor mapping checks occur before immutable publication; precondition tests pass. | closed |
| T-04-02 | Tampering | Undo/redo inverse history | high | mitigate | Inverses derive from pre-state, reversal is revision-bound, recovery replays semantics, and property tests prove restoration. | closed |
| T-04-03 | Repudiation | Command identity/outcome | medium | mitigate | Stable command, transaction, revision, attempt, modality, outcome, and safe error-code links are durable and tested. | closed |
| T-04-04 | Spoofing | Source modality | low | accept | Modality is explicitly diagnostic metadata and is not used as identity or authorization. See AR-05. | closed |
| T-05-01 | Tampering | Snapshot/log replay | high | mitigate | Recovery verifies size, schema, hashes, identities, revision continuity, operation meaning, and exact history before one final publication. | closed |
| T-05-02 | Denial of Service | Recovery scan/decode | high | mitigate | Hard record, byte, replay, and work budgets fail at N+1; hostile cases and the 200-page-equivalent benchmark pass. | closed |
| T-05-03 | Information Disclosure | Audit DTO/diagnostics | high | mitigate | Closed serialization/debug/error/WASM allowlists reject semantic text, command arguments, transcripts, audio, and arbitrary metadata. | closed |
| T-05-04 | Repudiation | Revision/provenance links | medium | mitigate | Immutable document, command, transaction, revision, schema, engine, migration, timestamp, and safe outcome lineage is kept in durable order. | closed |
| T-06-01 | Tampering | Inspector command dispatch | high | mitigate | DOM actions send explicit IDs and base revisions to the shared Rust validator; UI state updates only from successful result DTOs. | closed |
| T-06-02 | Tampering / Denial of Service | Browser commit/recovery | high | mitigate | Real-Chromium aborted-write, gap, hash, conflict, reload, and recovery tests retain the prior verified state on failure. | closed |
| T-06-03 | Information Disclosure | Audit DOM/accessibility output | high | mitigate | DOM and accessibility output are closed allowlists and negative-tested against sensitive sentinels in both locales/viewports. | closed |
| T-06-04 | Spoofing | Provenance presentation | medium | mitigate | Lineage is bound to the exact revision and PDF preview/export provenance is explicitly marked unavailable. | closed |
| T-06-05 | Elevation of Privilege | Deferred subsystem drift | low | accept | Structural contracts reject backend/auth/active-PDF/voice/editor surfaces in Phase 1; there is no privileged remote surface. See AR-06. | closed |

*All 25 register entries are closed. At ASVS L1, plan-authored mitigations were verified at implementation/test-reference depth as required by the short-circuit rule.*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-01 | T-01-03 | External registry unavailability is unavoidable; bounded fail-closed behavior preserves integrity and recoverability. | Phase 1 plan security profile | 2026-08-14 |
| AR-02 | T-01-04 | Public dependency metadata and local tool versions are intentionally reportable and contain no secret material. | Phase 1 plan security profile | 2026-08-14 |
| AR-03 | T-02-04 | The loopback inspector has no remote identity, authorization, or privileged action surface in this phase. | Phase 1 plan security profile | 2026-08-14 |
| AR-04 | T-03-04 | Unkeyed BLAKE3 is limited to deterministic identity/integrity and is never represented as authenticity. | Phase 1 plan security profile | 2026-08-14 |
| AR-05 | T-04-04 | Source modality is non-authoritative audit context and cannot grant rights. | Phase 1 plan security profile | 2026-08-14 |
| AR-06 | T-06-05 | Privileged remote subsystems are explicitly deferred and structurally excluded from the Phase 1 boundary. | Phase 1 plan security profile | 2026-08-21 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-08-25 | 25 | 25 | 0 | Codex / GSD secure-phase (ASVS L1) |

Evidence baseline: passing `npm run check` on commit `9e76093`, including dependency provenance/lock adversarial tests, structural boundary tests, 78 Rust tests, 27 unit/inspector tests, 13 real-Chromium tests, benchmark validation, and deterministic replay.

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer).
- [x] Accepted risks are documented in the Accepted Risks Log.
- [x] `threats_open: 0` is confirmed at the configured `high` blocking threshold.
- [x] `status: verified` is set in frontmatter.

**Approval:** verified 2026-08-25 — no blocking threats remain.
