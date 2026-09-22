# Phase 8 Research

**Confidence:** MEDIUM for the first bounded reconstruction slice; LOW for
general external-PDF semantics because a PDF often lacks paragraph and reading
order metadata.

## Current system state

- `crates/flow-core/src/pdf/reader.rs` owns bounded lazy PDF syntax reading,
  source hashes, page summaries, and privacy-safe diagnostics.
- `crates/flow-core/src/pdf/scene.rs` owns fixed-point page scenes with text,
  glyph rectangles, paths, clips, images, forms, links, annotations, and
  source object/operator provenance.
- `crates/flow-wasm/src/lib.rs` exposes a string-only verified `read_pdf` /
  `verify_pdf_reader_response` boundary; `web/src/pdf/pdf-reader-worker.ts`
  keeps reader cancellation and stale-result checks separate from the editor.
- `web/src/pdf/pdf-reader-panel.tsx` is intentionally read-only and visual;
  report/status DOM is the semantic surface. It does not publish imported
  scene text into the editor.
- `crates/flow-core/src/model/mod.rs` has a closed FlowDocument tree with
  paragraph/heading/list/table/image blocks and explicit provenance; no
  external-reconstruction provenance exists yet.
- `web/persistence/indexeddb-store.ts` already provides guarded immutable
  record patterns for local durable state, but imported PDF bytes need a
  separate source store so they cannot be confused with semantic assets.

## Constraints

### Technical

- Keep all geometry in `LayoutUnit`; no browser measurement may decide source
  ordering or candidate block boundaries.
- Bound page, element, text, OCR candidate, and serialized request/result
  sizes. Diagnostics carry codes and identities only, never source text or
  raw bytes.
- Use deterministic sort/group rules and stable hashes. A candidate result is
  accepted only when source hash, scene result hash, review decisions, and
  candidate result hash agree.
- Do not add an OCR engine or network dependency. The adapter contract must
  make provider absence and provider failure visible.

### Product

- External import is a reconstruction lane, not an inverse PDF transform.
- The original PDF remains inspectable beside the candidate. Uncertain text,
  unsupported visual content, and blocked active content are visible before
  acceptance.
- A candidate can be reviewed without mutating the existing semantic editor;
  accepting a candidate records external provenance and preserves the source
  binding/opaque-island sidecar.

### Delivery

- Plans lead with a vertical Rust candidate slice, then add the verified
  adapter/worker, comparison UI/source store, and finally regression gates.
- Existing Phase 1–7 contracts remain regression inputs. Missing qpdf,
  Poppler, OCR providers, target viewers, and external AT are reported as
  unavailable rather than converted into local passes.

## Research conclusions

1. Scene text already has enough fixed-point geometry and provenance to support
   deterministic single-column grouping without using DOM measurement.
2. A sidecar opaque-island lane is safer than inventing a new editable block
   for every unsupported PDF object; it preserves visual/source identity while
   keeping the FlowDocument schema closed.
3. OCR should be represented as a source-bound candidate protocol with a
   confidence threshold and review state. The host can provide a local/native
   adapter later without changing the Rust acceptance contract.
4. Source bytes must be retained outside semantic command DTOs. A guarded
   IndexedDB source store can preserve exact bytes while canonical candidate
   JSON carries only source identity and derived semantic content.

## Evidence boundary

No external OCR provider or reference PDF conversion tool is installed in the
declared environment. Phase 8 local evidence will prove contract, bounds,
determinism, source immutability, review gating, and browser comparison only.
