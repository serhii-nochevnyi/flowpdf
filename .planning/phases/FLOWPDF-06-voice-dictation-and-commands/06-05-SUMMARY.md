---
phase: FLOWPDF-06-voice-dictation-and-commands
plan: "05"
status: complete
completed: 2026-09-22
requirements: [VOIC-01, VOIC-03, VOIC-05, VOIC-06, QUAL-04]
---

# Phase 6 Plan 06-05 Summary

## Delivered

- Added a localized, persistent `VoiceControls` surface to the editor with
  explicit dictation/command radio modes, start/stop/cancel controls, and a
  visible microphone/recognition state.
- Kept interim and final recognition text outside the semantic document region
  as bounded, memory-only ghost/preview text. Mode changes and interim events
  never dispatch editor mutations.
- Routed final dictation through the existing `EditorController` voice
  transaction path and routed command speech through the Rust-backed resolver;
  source revision/hash/selection drift is rejected before command resolution or
  dispatch.
- Added an accessible `alertdialog` preview for intents whose Rust capability
  metadata requires explicit or conditional confirmation. Cancel and Escape
  clear the pending transcript without mutation; confirmation produces the
  normal controller transaction and status feedback.
- Added localized voice errors and action descriptions, persistent polite
  status, alert feedback, predictable focus return, and deterministic Chromium
  coverage for Ukrainian/English labels, unavailable recognition, interim
  ghost text, cancellation, confirmation, and one-transaction success.

## Verification

- `npm run typecheck` — passed.
- `npm run test:unit` — passed (82 tests).
- `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/voice-controls.browser.test.ts web/tests/editor-ui-states.browser.test.ts` — passed (6 tests).
- `npm run test:unit -- web/tests/voice-command.test.ts` — passed (5 tests).
- `git diff --check` — passed.

## Scope boundary

This slice does not claim a real browser speech service, microphone permission
success, or external screen-reader observation. Chromium tests inject a fake
recognition constructor so the local gate remains deterministic. Privacy and
phase-level regression closure remain in Plan 06-06.

## Commit

- `97c1786` — accessible voice controls, preview, confirmation, and feedback
