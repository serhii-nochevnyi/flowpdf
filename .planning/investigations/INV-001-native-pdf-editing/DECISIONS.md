# Decisions

<!-- Every accepted position is recorded IMMEDIATELY at the moment of the decision, not at the end. -->
<!-- Record format: -->
<!-- ## <decision, as an affirmative statement> -->
<!-- **Why:** ... -->
<!-- **What was rejected:** ... -->
<!-- **Scope fence:** what this decision explicitly does NOT cover -->
<!-- These sections become the locked decisions in the ADR when Gate 1 is closed. -->

## Native edits operate on a complete Rust-owned bounded source graph and emit a fresh PDF
**Why:** The current reader is lazy/read-only and the writer accepts only an owned graph. A complete graph makes reachability, preservation, and fresh serialization auditable before a derived result is returned.
**What was rejected:** Mutating source bytes in place, incremental updates, and rebuilding a page from the partial scene.
**Scope fence:** Only the admitted unencrypted classic-xref syntax subset is eligible. The original imported bytes remain immutable; encrypted, signed, malformed, or unrepresentable inputs are read-only.

## Preserve parsed reachable COS values and untouched stream bytes; fail closed on any unrepresentable reachable structure
**Why:** Unsupported page features must not disappear silently, while a generic bounded COS representation can retain passive unknown dictionary entries without interpreting them. Raw encoded streams avoid a decode/re-encode round trip for untouched data.
**What was rejected:** Treating opaque bytes as safe without validating object boundaries/references, and dropping unsupported objects during a scene rebuild.
**Scope fence:** Reachable syntax must still be fully bounded and represented. Redaction has stricter eligibility and may not retain hidden/opaque content. Unreachable objects are not copied into a fresh graph.

## Every PDF edit session and command is Rust-owned, immutable at its API boundary, and source/revision bound
**Why:** This follows existing Rust/WASM authority and the revision/hash-bound form-session pattern. It prevents stale commands from editing another import and gives the browser a verified result boundary.
**What was rejected:** Browser-side mutation of the scene or direct PDF-byte patching from TypeScript.
**Scope fence:** The session is request-local and derived; it does not replace the immutable source record or FlowDocument transaction model.

## Start native editing with plain Text annotations and a fresh-rewrite end-to-end tracer
**Why:** A note annotation is smaller and safer than editing arbitrary content streams, yet exercises imported graph ownership, mutation, serialization, verification, WASM, worker, and browser boundaries.
**What was rejected:** Beginning with text replacement, page graph cloning, or a mock-only UI path.
**Scope fence:** This is a first tracer only. Interactive scope later admits bounded text notes and non-signature text/checkbox AcroForm widgets; it excludes arbitrary annotation appearances and active actions.

## Text-island replacement is admitted only for reversible single-line source mappings with proven fit
**Why:** The current scene exposes mapped text and glyph positions but does not make arbitrary fonts, shaping, clipping, or layout safe to rewrite. Rejecting overflow avoids reflowing a fixed PDF page or covering neighbors.
**What was rejected:** Replacing any displayed Unicode run based solely on visual scene text, or silently shrinking/overflowing the replacement.
**Scope fence:** No general font substitution, line breaking, reflow, unsupported text modes/clips/transforms, or unproven encoding reversal.

## Page insertion creates blank pages only; same-document duplication and page operations preserve graph references transactionally
**Why:** Blank insertion avoids importing foreign resource graphs; same-document duplication can be validated against the already scanned graph and reference closure.
**What was rejected:** Cross-document page import in the first native editing milestone.
**Scope fence:** Deleting the final page is refused. An operation is rejected unless annotations, widgets, resources, destinations, and parent links remain valid.

## Redaction removes underlying supported content through a fresh complete rewrite and publishes only after every required validator passes
**Why:** PDF redaction is a two-stage process: marks identify content; applying them must remove the content and marks. Overlaying a box is not redaction. Independent extraction and raster evidence is required.
**What was rejected:** Incremental saving, clipping/covering content, claiming secure redaction from an internal writer check, or returning bytes when a required validator is missing/inconclusive.
**Scope fence:** Only fully recognized page content with supported text, vector, raster, and annotation handling is eligible. Unknown content, unsafe clips/masks/transforms, encryption, signatures, unsupported interactive/embedded content, or unscrubbed non-page content causes refusal. Strip metadata on redacted output; retain the immutable original.

## Phase 9 makes unsupported boundaries and external evidence explicit
**Why:** The local environment has no qpdf/Poppler executables or declared reference corpus; prior phase gates correctly record those checks as unavailable. Controlled tests cannot prove universal compatibility.
**What was rejected:** Installing host tools silently, inferring external evidence, or treating qpdf `--check` alone as a redaction proof.
**Scope fence:** The release lane may install/pin development-only validators and fixtures. No third-party PDF engine becomes a runtime dependency. The redaction implementation ticket requires a high-risk human checkpoint.
