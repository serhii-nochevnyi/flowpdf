# Phase 1 — UI Review

**Audited:** 2026-08-25  
**Reviewed commit:** `f9d1e19`  
**Baseline:** `01-UI-SPEC.md`  
**Screenshots:** `desktop.png`, `mobile.png`, `tablet.png`, `populated-desktop.png`, and `populated-mobile.png` under the ignored `.planning/ui-reviews/01-20260825-phase1-audit/` directory  
**Automated evidence:** 29 unit/inspector tests and 14 real-Chromium tests passed; the focused accessibility matrix covers Ukrainian and English at 320px and 1280px

---

## Pillar Scores

| Pillar | Score | Final finding |
|--------|------:|---------------|
| 1. Copywriting | 4/4 | Ukrainian and English resources are key-identical, runtime/static fallbacks are localized, and copy results use natural noun-based wording. |
| 2. Visuals | 4/4 | Empty and populated states have clear hierarchy; audit evidence is readable without panning, and the zero-row frame collapses completely. |
| 3. Color | 4/4 | Semantic CSS tokens are used consistently; the control boundary is 7.58:1 on white and status colors retain text labels. |
| 4. Typography | 4/4 | The implementation uses only the approved font sizes, weights, line heights, and monospaced identifier role. |
| 5. Spacing | 4/4 | Shell, cards, actions, responsive stacking, and feedback placement use the declared spacing scale with 44px minimum targets. |
| 6. Experience Design | 4/4 | Busy controls are truthful, lifecycle/copy announcements are isolated, mobile feedback remains in view, and audit/accessibility semantics are preserved. |

**Overall: 24/24**

No blocker, warning, or informational recommendation remains in the Phase 1 UI scope.

---

## Closed Findings

| Prior finding | Resolution | Evidence |
|---------------|------------|----------|
| Enabled controls could silently no-op while a command was busy. | Every conflicting action is disabled during the transaction; operative read-only copy controls remain enabled. | Unit regression plus Chromium accessibility suite. |
| The 720px audit table clipped outcome and command identity inside the 360px inspector. | Each audit row is a labelled stacked record while retaining native table, `th`, `headers`, caption, and source-order semantics. | Desktop audit wrapper `308/308`; mobile `252/252`; every cell is header-linked. |
| Colors were repeated literals and the secondary control boundary was below 3:1. | Semantic CSS variables now own surfaces, text, borders, accent, and statuses; the control border is `#475569`. | Computed-color assertions and 7.58:1 measured boundary contrast. |
| Page title, startup failure, and no-script fallback bypassed localization. | Runtime title/error use the selected locale; the static Ukrainian no-JavaScript title and message are locked to the localization resources. | Raw-index parity test and real English `main.ts` bootstrap test. |
| Heading and spacing roles diverged from the design contract. | Empty-state and command headings use approved roles; 12px one-off spacing was removed. | Computed typography assertions and source audit. |
| Copy feedback could overwrite an in-flight document status or finish out of order. | Copy success and failure use one separate visible status/alert container; a generation token discards stale clipboard completions. | Delayed success, failure, and out-of-order unit regression. |
| Copy feedback was off-screen after a mobile hash copy. | The one feedback container moves into the invoking copy definition and uses bounded `scrollIntoView` only below 480px. | At 320×900 the hash feedback was adjacent at y=487.9–511.9 and intersected the viewport; Chromium geometry assertion covers the same flow. |
| Copy success read as “Скопійовано Копіювати хеш.” / “Copied Copy hash.” | Dedicated localized noun keys produce “Скопійовано хеш ревізії.” and “Revision hash copied.” | Exact bilingual copy assertions and final manual browser check. |
| Zero audit entries left an empty framed strip. | The audit wrapper is hidden until at least one row exists and the empty-state message remains visible. | Computed `display: none`, 0px height, then populated semantic-table restoration test. |

---

## Detailed Assessment

### 1. Copywriting — 4/4

- Visible Ukrainian copy is backed by a key-identical English resource.
- Button labels describe concrete operations; success, failure, conflict, recovery, and provenance messages state the outcome.
- Document ID and revision-hash copy success use grammatical noun keys rather than reusing imperative button labels.
- The static Ukrainian no-JavaScript title/message and the English runtime override are both explicitly verified.

### 2. Visuals — 4/4

- The header, command surface, current-document card, and revision inspector form a restrained professional hierarchy without ornamental imagery, gradients, or shadow noise.
- The desktop shell preserves the flexible primary column and 360px inspector; tablet and mobile stack in source order.
- Stacked audit records expose all six values at first view without horizontal panning.
- The zero-row state shows explanatory copy without an empty border; populated audit evidence restores the framed records.

### 3. Color — 4/4

- All functional colors are semantic custom properties.
- Primary/secondary text, card boundaries, controls, focus, success, and failure maintain their intended roles.
- Pending is neutral, completion is success, and errors use the error token; every state also has text.
- The default control boundary exceeds the 3:1 non-text contrast requirement.

### 4. Typography — 4/4

- The page uses the approved system sans-serif stack with 14, 16, 20, and 28px roles only.
- Heading and label weights remain within the approved 400/600 set.
- IDs and hashes use the approved monospaced role and retain their full accessible value despite visual truncation.
- No visible text falls below 14px.

### 5. Spacing — 4/4

- Page, card, grid, action, and feedback spacing follows the 4/8/16/24/32/48px scale.
- Buttons retain at least 44px height and action groups wrap without collision.
- At 320px the document and audit surfaces remain exactly viewport-bounded (`320/320` page width; `252/252` audit width).
- Feedback occupies a stable 24px line and is placed in the active metadata definition.

### 6. Experience Design — 4/4

- Empty, pending, populated, conflict, recovery, zero/one/many audit, long identifier, and narrow viewport states have explicit behavior.
- Native controls, visible focus, labelled landmarks, table header associations, machine-readable timestamps, full accessible identifiers, and keyboard undo/redo are preserved.
- The document lifecycle status remains authoritative while independent copy feedback succeeds or fails.
- The first `role="alert"` remains the lifecycle alert for backward-compatible recovery/conflict behavior; the copy alert is separate and visible.
- The interface explicitly states that PDF preview/export provenance is not available in Phase 1.

---

## Final Evidence

- `npm run check`: passed in 21.07 seconds.
- Rust: 78 tests passed; formatting and warnings-as-errors Clippy passed.
- Unit/inspector: 29/29 passed.
- Accessibility matrix: 4/4 passed.
- Full Chromium suite: 14/14 passed.
- Deterministic canonical and migration replay: two rounds passed.
- Independent exact-diff UI rereview: clean, with focused typecheck, inspector 8/8, and accessibility 4/4 reruns.
- Manual final checks: 1280×900 and 320×900 had no page or audit horizontal overflow; targets remained at least 44px high; zero audit frame collapsed; mobile hash feedback was adjacent, visible, and left the lifecycle status unchanged.

## Files Audited

- `.planning/phases/FLOWPDF-01-durable-flow-foundation/01-UI-SPEC.md`
- `web/index.html`
- `web/src/main.ts`
- `web/src/foundation-inspector.ts`
- `web/src/styles.css`
- `web/src/i18n/uk.ts`
- `web/src/i18n/en.ts`
- `web/tests/foundation-inspector.test.ts`
- `web/tests/accessibility.browser.test.ts`
- `web/tests/walking-skeleton.browser.test.ts`
- `web/tests/recovery.browser.test.ts`

## Recommendation Count

- Blockers: 0
- Warnings: 0
- Informational recommendations: 0

---

_Reviewer: Codex UI audit with independent exact-diff rereview_  
_Final verdict: PASS_
