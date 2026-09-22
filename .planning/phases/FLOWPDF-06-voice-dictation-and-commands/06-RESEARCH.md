# Phase 6 Research: Voice Dictation and Commands

**Researched:** 2026-09-22

## Existing repository seams

1. `SourceModality::Voice` is already a closed transaction modality. The
   capability catalog carries a `FutureVoiceBinding` for every mutation family,
   so voice can reuse the same command vocabulary instead of adding a parallel
   semantic path.
2. `EditorController` serializes accepted canonical JSON/history through
   `apply_command`, persists the returned commit, performs a Rust re-query, and
   publishes only the verified revision/hash. Its `replaceText`, structural,
   formatting, undo, redo, and editor-session methods are the correct dispatch
   seam for voice.
3. `EditorStore` already provides a stable accepted snapshot, current logical
   selection, status/alert channels, and a separate noncanonical editor-session
   projection. Layout/PDF/form work is derived and must not be made a voice
   prerequisite for a mutation.
4. `FormSessionCoordinator` owns source-bound noncanonical field values and
   exposes serialized Rust-validated set/clear operations. Voice field actions
   must target this coordinator only after resolving an accepted field identity.
5. Existing UI tests use fake WASM/controller seams and real Chromium; a fake
   `SpeechRecognition` constructor can exercise recognition event ordering
   without requiring a microphone or browser speech service.

## Browser API facts

The Web Speech API specification defines `SpeechRecognition` as a secure-context,
window-exposed API with `start()`, `stop()`, `abort()`, `lang`, `continuous`,
`interimResults`, `onresult`, `onerror`, `onstart`, and `onend`. `stop()` ends
capture and finalizes interim results; `abort()` stops recognition and discards
unfinalized results. Each `SpeechRecognitionResult` exposes `isFinal`, which is
the boundary needed to keep interim text out of the document.

Source: [Web Speech API SpeechRecognition reference](https://github.com/webaudio/web-speech-api/blob/main/_autodocs/api-reference/speech-recognition.md)

The implementation must still feature-detect prefixed constructors and handle
permission/service errors because the API is not uniformly available across
the supported browsers. No code should infer microphone permission from the
presence of the constructor.

## Proposed ownership boundary

```text
SpeechRecognition events
  -> ephemeral VoiceSession (TypeScript, no persistence)
  -> final dictation or bounded transcript request
  -> Rust voice intent resolver (command mode only)
  -> existing editor/form command seam with SourceModality::Voice
  -> normal durable commit + Rust re-query + accessible feedback
```

Rust receives a bounded locale/mode/transcript/selection/revision request and
returns a structured intent with risk and confirmation metadata. It does not
return raw transcript analytics or mutate canonical state. TypeScript may show
the returned intent and ask for confirmation, but accepted execution still
passes through the existing typed command boundary.

## Initial command catalog

The first catalog should cover at least one safe path in each required group:

- navigation: next/previous field or next/previous logical target;
- formatting: bold, italic, underline for the accepted selection;
- editing: delete/replace the accepted selection and insert a page break;
- history: undo and redo;
- fields: clear a named/current field and move between valid fields.

Phrase matching is exact after bounded Unicode whitespace/case normalization,
with Ukrainian and English entries. Unknown, conflicting, or under-specified
phrases return an explicit unsupported/ambiguous result rather than guessing.

## Risks and fences

- Recognition can emit the same result index repeatedly; the adapter must track
  `resultIndex` and only append new final segments.
- A continuous session can produce multiple final segments. Push-to-talk stop
  coalesces the final segments into one transaction; no segment is dispatched
  independently.
- The document or selection can change while recognition is active. The final
  commit must compare the captured revision/hash/selection identity and fail
  closed on drift.
- Command speech must never be routed through dictation text insertion, and
  dictation text must never be sent to the command resolver.
- Browser speech services may be remote and are outside FlowPDF's telemetry
  ownership. The app stores no audio and no raw transcript history by default.
- `getUserMedia` is not needed for the Web Speech API path; adding a separate
  microphone stream would broaden permissions and the privacy boundary without
  product value for this MVP.
