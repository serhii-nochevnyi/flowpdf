# Phase 4 Plan 04-06 Summary

**Status:** Complete (plan); Phase-level closure remains open
**Completed:** 2026-09-21

## Delivered

- Added the exact Phase 4 validation manifest with twelve local rows (eleven
  executable rows plus one terminal orchestration row) for the owned
  COS/resource/provenance/WASM/worker/preview slices and four separate
  structural, extraction, raster, and target-viewer reference rows.
- Added a deterministic, pinned `check-phase4` runner. It fingerprints the
  validation contract and implementation inputs, executes local rows with
  privacy-safe allowlisted diagnostics, preserves the Phase 1/2/3 regression
  gates, and never promotes a missing reference tool or fixture to `pass`.
- Added smoke contracts for manifest completeness, future-scope rejection,
  pinned-cache/no-install behavior, safe diagnostics, unavailable references,
  and revision-safe stale-result guards.
- Promoted the retained boundary contract to the current Phase 4 surface:
  only `web/src/pdf/**` and the three string-only PDF WASM exports are admitted;
  deferred PDF-looking paths nested under other ownership areas remain blocked.
- Refreshed the tracked Phase 1 recovery benchmark artifact after the new
  `flow-core` source manifest changed, so the retained aggregate gate validates
  current source rather than stale evidence.

## Verification

- `node --test tests/contracts/phase1-boundary.test.mjs tests/contracts/phase2-boundary.test.mjs` — 31 tests passed.
- `npm run check:phase3` — all 16 local tasks passed, including the aggregate
  Phase 1 gate, clippy, scale smoke, and boundary regression rows.
- `npm run check:phase4:smoke` — 6 contract tests passed.
- `npm run check:phase4` — 11/11 local tasks passed; all four reference rows
  reported `unavailable` because the checked-in Phase 4 PDF fixture and the
  required external tools/viewer are absent in this environment.
- `git diff --check && npm run check:phase4:smoke && npm run check:phase4 &&
  npm run check:phase4` — passed; both full runs retained the same
  11/11-local-pass and 4/4-reference-unavailable distinction.

## Scope boundary and closure decision

The local gate proves only the owned Phase 4 contracts implemented so far:
bounded COS/page structure, derived display-list/resource preparation,
reproducibility and exact owned-source recovery, string-only WASM/worker
export, and visual-only virtualized preview. It does not claim general PDF
import, AcroForm, encryption/signing, OCR, voice, PDF/A, PDF/UA, or target-viewer
compatibility. The Phase 4 roadmap checkbox remains open until reference PDF
structural/text/raster evidence is available; the inherited Edge/Windows
screen-reader checkpoint also remains outstanding.
