# Phase 1: Durable Flow Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents. Decisions are captured in CONTEXT.md; this log preserves the alternatives considered.

**Date:** 2026-08-14  
**Phase:** 1-Durable Flow Foundation  
**Areas discussed:** Canonical schema and assets, Transactions and revisions, Logical anchors, Durability and audit

---

## Canonical Schema and Assets

| Option | Description | Selected |
|--------|-------------|----------|
| Canonical JSON with derived package adapters | Human-inspectable deterministic semantic truth; ZIP/binary remain adapters | ✓ |
| ZIP package as the primary object | Package shape becomes the in-memory authority | |
| Binary schema first | Optimize size/performance before compatibility contracts are proven | |

**Choice:** Canonical JSON with explicit sequential migrations and content-addressed assets.  
**Notes:** Auto-selected recommended default. Published schema and migration chains are treated as costly compatibility contracts.

---

## Transactions and Revisions

| Option | Description | Selected |
|--------|-------------|----------|
| Typed transactions with explicit inverses | Shared command boundary, atomic undo, revision preconditions | ✓ |
| Snapshot-only undo | Replace history with full document copies | |
| Mutable model with UI-managed undo | Each adapter owns inconsistent mutation behavior | |

**Choice:** Typed commands create immutable atomic transactions and structured errors reject stale targets.  
**Notes:** Auto-selected recommended default. Silent rebasing is deferred until a collaboration model exists.

---

## Logical Anchors

| Option | Description | Selected |
|--------|-------------|----------|
| Stable node anchor with offset and affinity | Independent of rendering and pagination | ✓ |
| DOM Range serialization | Couples persisted state to transient browser structure | |
| Absolute document character index | Expensive and unstable after structural changes | |

**Choice:** Stable node ID, explicit UTF-16 offset/affinity at the browser boundary, and transaction-produced mappings.  
**Notes:** Auto-selected recommended default. Deleted targets invalidate rather than guess.

---

## Durability and Audit

| Option | Description | Selected |
|--------|-------------|----------|
| Snapshots plus append-only log behind an adapter | Recoverable, testable, local-first and backend-neutral | ✓ |
| Complete JSON after every keystroke | High write amplification and no event provenance | |
| Server-only persistence | Blocks offline architecture spike and creates premature backend scope | |

**Choice:** `DocumentStore` port with canonical snapshots, idempotent log replay, IndexedDB browser adapter and privacy-minimized audit.  
**Notes:** Auto-selected recommended default. Raw audio and document text are excluded from default audit records.

## the agent's Discretion

- Internal crate/module names, hash implementation, snapshot cadence, IndexedDB store names, and error wording within the locked contracts.

## Deferred Ideas

- Compact binary encoding, real-time CRDT collaboration, remote synchronization, rich editing, layout and PDF work remain in their roadmap phases.

