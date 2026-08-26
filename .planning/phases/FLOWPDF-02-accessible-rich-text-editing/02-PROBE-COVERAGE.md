# Phase 2 — Spec-less Probe Coverage

> Deterministic edge-probe floor plus the autonomous resolution pass consumed by the Phase 2 planner. The phase has no standalone SPEC, so every resolved predicate below must be lifted into PLAN `must_haves.truths`; the prohibitions remain descriptor-less and therefore flagged-unverified.

## Edge Coverage

| Requirement | Category | Status | Verification | Acceptance predicate |
|-------------|----------|--------|--------------|----------------------|
| EDIT-01 | empty | resolved | explicit | An empty text block has exactly one caret position at UTF-16 offset 0; nonzero endpoints reject without changing document or selection. |
| EDIT-01 | encoding | resolved | explicit | Accepted text endpoints are exactly UTF-16 offsets corresponding to pinned UAX #29 extended-grapheme boundaries over the stored, non-normalized sequence; surrogate, combining-mark, emoji-ZWJ, and regional-indicator interiors reject without snapping in native Rust and WASM. |
| EDIT-01 | boundary | resolved | explicit | Text positions accept exactly `0..=utf16_len` at grapheme boundaries; atomic image/table/page-break nodes expose only offset 0/Forward before and 1/Backward after, with directional whole-atom selection 0↔1 and no interior caret. |
| EDIT-01 | ordering | resolved | explicit | Anchor/focus direction and affinity survive validation and formatting; command-local start/end normalization never replaces the published directional selection. |
| EDIT-02 | empty | resolved | explicit | Insertion into an empty paragraph succeeds; replacing a nonempty selection with an empty string is one atomic deletion; an empty collapsed replacement is a no-op without revision/history; empty `compositionend.data` alone is not cancellation. |
| EDIT-02 | encoding | resolved | explicit | Keyboard, plain-text paste, and IME preserve the exact Unicode scalar sequence without NFC/NFD conversion; malformed UTF-16 rejects and Ukrainian, English, decomposed text, apostrophes, non-BMP text, and emoji use the same Rust command path. |
| EDIT-02 | idempotency | resolved | explicit | Redelivery of one command ID/composition commit produces at most one revision and one undo entry; a duplicate rejects without applying text twice. |
| EDIT-02 | concurrency | resolved | explicit | Of two mutations based on one accepted revision, at most one commits; the other returns the stable stale-revision error without rebase, retry, relocation, DOM adoption, or publication, and a changed revision invalidates active composition. |
| EDIT-02 | boundary | resolved | explicit | Composition is bounded at 64 KiB UTF-8 and 32,768 UTF-16 units; plain-text paste is bounded at 256 KiB UTF-8; max succeeds and max+1 rejects before canonical mutation and within the 1 MiB serialized command envelope. |
| EDIT-03 | adjacency | resolved | explicit | Merge is allowed only for immediate compatible siblings in one allowed container—paragraph/paragraph, equal-level heading/heading, or compatible sibling list-item paragraphs; atomic/incompatible/intervening nodes cause atomic rejection. |
| EDIT-03 | empty | resolved | explicit | Splitting an empty paragraph creates one empty sibling; heading start/end and empty-list-item Enter follow the locked matrix; removing the last editable block creates one empty paragraph in the same transaction. |
| EDIT-03 | encoding | resolved | explicit | Every split/merge endpoint is a grapheme-valid UTF-16 boundary and the operation preserves exact text bytes, marks, and stored normalization. |
| EDIT-03 | ordering | resolved | explicit | Split partitions A at s into A[0..s] then B[s..]; merge produces A+B; preorder, stable IDs, directional selection, field anchors, and inverse reconstruction remain exact. |
| EDIT-03 | boundary | resolved | explicit | Start/end split behavior follows the complete paragraph/heading/list/table-cell matrix; first/last or incompatible merge rejects without guessing a neighbor. |
| EDIT-03 | idempotency | resolved | explicit | Duplicate split/merge IDs create/remove no additional block; exact inverse restores canonical bytes, IDs, selection/session endpoints, field-anchor states, and history cursor. |
| EDIT-04 | adjacency | resolved | explicit | Range formatting changes exactly the grapheme clusters in the normalized logical range, never adjacent clusters; equal adjacent runs merge and unsupported atomic blocks return an explicit result. |
| EDIT-04 | empty | resolved | explicit | Collapsed inline formatting changes only Rust-owned, revision-bound pending typing attributes without a document revision/history entry; compatible block formatting on an empty block remains a normal document mutation. |
| EDIT-04 | ordering | resolved | explicit | Forward and reverse selections format the same normalized range while preserving anchor/focus direction; compatible blocks and canonical runs remain in document/text order. |
| EDIT-04 | idempotency | resolved | explicit | Formatting commands set explicit target values; setting an active value is a canonical no-op, duplicate IDs cannot toggle twice, and a UI toggle is resolved once by Rust against its base revision. |
| EDIT-04 | concurrency | resolved | explicit | Formatting/session actions carry base revision and logical selection; stale actions reject without optimistic state, relocation, or partial formatting, and pending attributes are recomputed or invalidated on selection/revision change. |
| EDIT-04 | boundary | resolved | explicit | Authoring accepts heading 1–6; font size 6,000–288,000 millipoints; spacing 0–144,000 millipoints; alignment start/center/end/justify; language uk-UA/en-US; semantic font IDs noto-sans/noto-serif/noto-sans-mono; and list none/ordered/unordered; invalid/max+1 values reject without clamping. |
| EDIT-04 | encoding | resolved | explicit | Colors enter as exact uppercase `#RRGGBB` and persist as typed RGB; language/font values are closed IDs rather than browser CSS strings; unsupported values reject without fallback/substitution. |
| EDIT-05 | adjacency | resolved | explicit | Structural insertion uses one explicit parent/index or logical boundary; atomic nodes are not consumed by ordinary text deletion/implicit merge; page-break insertion retains an editable paragraph after it. |
| EDIT-05 | empty | resolved | explicit | New/emptied documents retain one paragraph; tables are at least 1×1 with an editable paragraph per cell; new images require nonempty description or explicit decorative choice; missing legacy alt remains `MissingLegacy`, never invented decorative intent. |
| EDIT-05 | ordering | resolved | explicit | List siblings and inserted blocks retain logical order; table cells navigate row-major; row/column changes retain unaffected IDs/order and focus the nearest survivor; undo restores exact prior subtree order. |
| EDIT-05 | idempotency | resolved | explicit | Duplicate command IDs cannot insert a second list/image/table/page break, repeat row/column edits, or duplicate asset references; Set operations already at their target are no-ops. |
| EDIT-05 | concurrency | resolved | explicit | Structural commands validate revision, selection, parent/index, and staged-asset receipt; stale targets reject atomically and asset bytes plus document reference become durable together without orphan publication. |
| EDIT-05 | boundary | resolved | explicit | Enforce image ≤8 MiB, ≤8,192 per axis, ≤24M pixels, ≤96 MiB decoded; table ≤50×20/1,000 cells; list nesting ≤8; normalized runs ≤4,096; every zero/min/max/max+1 case has a stable result. |
| EDIT-05 | encoding | resolved | explicit | Rust accepts only bounded PNG/JPEG signatures and successful bounded decode, not filename/MIME; alt text preserves exact Unicode and staged bytes cross WASM through an opaque receipt rather than the semantic command DTO. |
| QUAL-03 | adjacency | resolved | explicit | The generated closed command catalog maps every Phase 2 mutation family to at least one labeled visible control and keyboard route; any voice-exposed intent resolves to the same command/risk metadata. |
| QUAL-03 | empty | resolved | explicit | The parity gate is non-vacuous: an empty catalog, missing binding, or voice-only mutation fails; no mutation is exempt because voice capture is deferred. |
| QUAL-03 | ordering | resolved | explicit | Equivalent ordered intent traces through keyboard, visible UI, and the future voice-intent seam yield equal semantic command DTO sequences, canonical hashes, revisions, and undo histories except modality-only audit metadata. |
| QUAL-04 | unclassified→semantic/accessibility | resolved | explicit | Rust projects valid existing Phase 1 field descriptors in logical order with label/name, kind, required/read-only state, and value summary; `LegacyInvalid`/`TargetDeleted` descriptors remain keyboard reachable in a labeled placement-review group with original target/reason; document content, statuses, errors, and confirmations remain semantic and localized; no Phase 2 field author/fill command exists. |

**No-silent-drop equality:** 33 surfaced = 33 resolved explicit + 0 backstop + 0 unresolved + 0 dismissed.

## Prohibitions

These are deliberately descriptor-less (`check_kind`, `check_target`, `check_rule`, and fixtures are absent). The planner must serialize them under `must_haves.prohibitions`, not `truths`, with `status: unverified`, `flagged: true`, and the stated verification tier.

| ID | Requirements | Verification tier | Statement |
|----|--------------|-------------------|-----------|
| P2-01 | QUAL-03 | test | MUST NOT grant a voice-originated mutation broader authority, weaker validation/confirmation, different undo semantics, or a route unavailable through keyboard and visible UI. |
| P2-02 | EDIT-05, QUAL-04 | test | MUST NOT invent alt text or silently classify a legacy image with empty/unknown alt semantics as decorative; it remains `MissingLegacy`/needs-review until an explicit user accessibility decision. |
| P2-03 | QUAL-04 | judgment | MUST NOT use deferred field authoring to hide, discard, or make existing Phase 1 field descriptors unreachable to screen-reader users, nor claim QUAL-04 closed while field navigation or the external Edge/Windows/screen-reader evidence is absent. |

**Prohibition equality:** 3 kept = 3 descriptor-less flagged-unverified + 0 silently dropped.
