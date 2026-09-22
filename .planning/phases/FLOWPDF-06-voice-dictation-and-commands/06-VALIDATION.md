# Phase 6 Validation Manifest

Phase 6 is locally reproducible without a real microphone or remote speech
service. Browser recognition tests inject a typed fake `SpeechRecognition`
constructor; the production adapter still feature-detects the standard and
Chromium-prefixed browser constructors.

| ID | Local lane | Command |
| --- | --- | --- |
| 06-01-01 | Rust bounded voice resolver | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test voice -- --nocapture` |
| 06-01-02 | Rust/WASM voice export | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test voice_exports -- --nocapture` |
| 06-02-01 | Typed bridge and controller atomicity | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/voice-command.test.ts web/tests/editor-controller.test.ts` |
| 06-03-01 | Recognition unit lifecycle | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/voice-recognition.test.ts` |
| 06-03-02 | Recognition Chromium lifecycle | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/voice-recognition.browser.test.ts` |
| 06-04-01 | Field-session and command routing | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/form-session.test.ts web/tests/voice-command.test.ts web/tests/editor-controller.test.ts` |
| 06-05-01 | Voice surface type/unit lane | `npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/voice-command.test.ts` |
| 06-05-02 | Accessible voice UI Chromium lane | `PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/voice-controls.browser.test.ts web/tests/editor-ui-states.browser.test.ts` |
| 06-06-01 | Boundary, privacy, and diff contracts | `node --test tests/contracts/phase6-gate.test.mjs tests/contracts/phase1-boundary.test.mjs && git diff --check` |
| 06-06-02 | Full unit regression | `npm run test:unit` |
| 06-06-03 | Full Rust core/WASM regression | `RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet` |
| 06-06-04 | Web/WASM production build | `npm run build:web` |
| 06-06-05 | Full Chromium regression | `npm run test:browser` |
| 06-06-06 | Existing project check runner | `npm run check` |
| 06-06-07 | Orchestrated rerun and planning check | `npm run check:phase6 && npm run check:phase6 && npm run check:planning` |

The final row is orchestration-only and is not recursively executed by the
phase runner. Run `npm run check:phase6` twice after the local rows pass; the
second run is the deterministic rerun evidence. During implementation before
the 06-06 summary exists, `npm run check:phase6 -- --allow-open-plan` may be
used; the final command requires every Phase 6 plan summary to be complete.

## Evidence boundary

| Evidence lane | Status | Truthful boundary |
| --- | --- | --- |
| Rust/WASM resolver and export | pass locally | Exact bounded intents and typed string boundary are tested. |
| Controller, form-session, recognition, and UI | pass locally | Unit and deterministic Chromium tests cover source fences, one-transaction dispatch, preview/cancel/confirm, and semantic status/error surfaces. |
| Browser speech permission/service | unavailable | No real microphone permission or remote speech-service result is claimed; fake recognition is used for deterministic tests. |
| External screen-reader observation | unavailable/outstanding | DOM roles, names, live regions, focus, and alertdialog semantics are asserted locally; Windows/Edge and external AT evidence remain separate release inputs. |
| Raw audio/transcript persistence | pass by boundary | Voice source inspection rejects storage, network/analytics, and independent audio-capture surfaces; pending transcript state is memory-only. |

