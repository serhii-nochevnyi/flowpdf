# Phase 6 Context: Voice Dictation and Commands

## Goal

Add push-to-talk voice input as an additional, explicit modality over the
existing Rust transaction/editor/form seams. Voice must never become a second
semantic authority, an always-listening background service, or a route around
revision, confirmation, accessibility, undo, and privacy rules.

## Locked scope

- Dictation and command mode are visibly separate. The default interaction is
  push-to-talk; there is no ambient activation.
- Interim recognition is ephemeral ghost text only. A stopped/ended dictation
  utterance becomes one `SourceModality::Voice` transaction at the captured
  revision and selection, or is rejected as stale without relocating text.
- Command speech is parsed through a bounded Rust/WASM allowlist. TypeScript
  owns the browser recognition API and UI mechanics but does not interpret
  free-form speech into semantic commands.
- Existing `apply_command`, `apply_editor_session`, form-session, capability,
  persistence, and undo/redo boundaries remain authoritative. Every accepted
  voice mutation is a normal transaction with inverse/audit/revision checks.
- Destructive, ambiguous, export, flatten, and signing-related intents require
  a visible/accessible preview or confirmation. Unsupported or ambiguous speech
  is reported without mutation.
- Raw audio and raw transcript analytics are not persisted by default. Voice
  session state, interim text, pending preview, and failure diagnostics are
  memory-only browser state unless the user commits text through the normal
  document transaction.

## Out of scope

- Always-listening activation, remote voice analytics, server-side transcript
  storage, custom speech models, speaker identification, or guaranteed offline
  recognition.
- General natural-language planning. The first command catalog is exact,
  locale-aware, bounded phrases mapped to structured intents.
- Replacing keyboard, IME, visible controls, or semantic DOM with voice.

## Human-facing behavior

The editor exposes a persistent microphone state, an explicit mode switch, a
start/stop control, an ephemeral interim transcript region, and an accessible
command preview/confirmation when needed. Recognition unavailable, permission,
network, no-match, and aborted states are visible and actionable. Success and
failure use the existing polite status/alert channels; optional speech output
is not required for the initial slice.
