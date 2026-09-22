---
phase: quick
plan: 260909-mhv
type: execute
wave: 1
depends_on: []
files_modified:
  - README.md
autonomous: true
requirements:
  - FLOW-01
  - QUAL-08
estimate:
  tokens: 4000
  raw_tokens: 4000
  tasks: 1
  confidence: low
must_haves:
  truths:
    - A repository visitor can distinguish the working foundation inspector and canonical model from planned document-editing features.
    - A developer can identify the exact local build prerequisites and why a fresh clone is not yet a portable ready-to-run installation.
    - README commands and local links resolve to existing repository scripts and files.
  artifacts:
    - path: README.md
      provides: Public English project overview, architecture, status, repository map, and qualified development instructions
  key_links:
    - from: README.md
      to: package.json
      via: Existing npm script names
    - from: README.md
      to: .planning/STATE.md
      via: Implemented-versus-planned status
    - from: README.md
      to: scripts/verify-wasm-bindgen-tool.mjs
      via: Explicit toolchain integrity prerequisites
---

<objective>
Create an evidence-backed English README for the public FlowPDF repository. This is a documentation-only quick task supporting FLOW-01 and QUAL-08; it neither claims new product functionality nor changes requirement completion.

Purpose: Explain the product and its current development boundary without misleading fresh-clone users.
Output: README.md. Existing Phase 2 implementation continues separately under its established plans.
</objective>

<execution_context>
@/Users/serhii/.codex/gsd-core/workflows/execute-plan.md
@/Users/serhii/.codex/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/STATE.md
@.planning/ROADMAP.md
@.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-03-SUMMARY.md
@package.json
@rust-toolchain.toml
@config/dependency-provenance.json
@scripts/node-version.mjs
@scripts/verify-wasm-bindgen-tool.mjs
@scripts/build-web.mjs
@scripts/serve-inspector.mjs
@vite.config.ts
@.gitignore
</context>

<tasks>
<task type="auto">
  <name>Task 1: Document the product, proven foundation, and qualified local developer workflow</name>
  <files>README.md</files>
  <action>
Create a concise but useful English README. Lead with FlowPDF's purpose: a web-first Ukrainian/English document editor combining semantic rich-text flow, eventual reflow, fillable forms, and voice control without a commercial PDF SDK. Immediately state that the repository is an in-development engine and foundation inspector, not yet a complete PDF editor.

Explain why semantic FlowDocument is authoritative for rich-text editing, while layout fragments, display lists, and fixed-layout PDF object graphs have separate responsibilities. Rust owns semantics and targets native/WASM; TypeScript owns browser presentation and physical I/O, with Web Workers isolating core work. Clearly label layout/pagination, owned PDF writing/reading, source round-trip, form authoring, and voice as the intended architecture, not already delivered runtime functionality. External PDF reconstruction is best-effort and provenance-aware, not universally lossless; preserving unsupported content and immutable originals is a design requirement.

Use an implemented/planned status section grounded in Phase 1 and completed Phase 2 Plan 03. Describe canonical schema version 2 rich structures and migrations, stable identity/canonical hashing, transactional lifecycle and existing undo/recovery foundations, browser IndexedDB persistence, worker/WASM bridge, and foundation inspector. Explicitly distinguish schema support for lists/tables/images/field anchors from working rich-text editing UI or PDF AcroForm export. Say Unicode cursor/selection/editor-session work is next; avoid claiming shaping, document-wide repagination, selectable PDF export, external import, voice, complete accessibility validation, or production readiness.

Add a compact repository map for crates/flow-core, crates/flow-wasm, web, fixtures, scripts, config, artifacts, and .planning. Link relevant tracked documentation, manifests, and scripts with repository-relative Markdown links. Do not add an invented license, CI badge, hosted demo, install script, or unsupported hosting claim.

Document exact Node 24.10.0, npm 11.9.0, Rust 1.97.1, wasm32-unknown-unknown, rustfmt/clippy, and wasm-bindgen-cli 0.2.108 prerequisites. Explain that current commands use workspace-local work/toolchains/rustup and work/toolchains/cargo rather than an arbitrary global Cargo installation. The tool verifier requires the approved binary bytes, Cargo installation receipt at work/toolchains/cargo/.crates2.json, and successful tracked provenance; the only configured binary target is aarch64-apple-darwin. A same-version rebuilt binary or another platform is not automatically approved. Never suggest bypassing the verifier or replacing approved checksums casually. Generated WASM/glue in web/generated, local toolchains/browser binaries under work, node_modules, target, and dist are ignored and absent from a fresh clone. State plainly that a portable fresh-clone bootstrap is not provided yet; npm ci alone cannot satisfy the Rust/tool-integrity prerequisites.

Present existing commands only, grouped by purpose and qualified as requiring the prepared local environment: npm ci for locked JavaScript dependencies; npm run inspector for the working inspector at http://127.0.0.1:4173 (FLOWPDF_INSPECTOR_PORT can override it); npm run build:wasm before npm run dev when using Vite; npm run build:web, npm run typecheck, npm run test:unit, npm run test:browser, npm test, and npm run check as appropriate. Explain browser suites additionally need the pinned Chromium installation in work/playwright. Do not imply npm run dev generates WASM. Mention the full check includes provenance/network-sensitive work and is broader than documentation validation. Keep development servers local, consistent with the unresolved Browser Mode dependency exposure note in STATE.md.

Perform a docs-only review of every claim, local link, and command against the cited files. Do not modify code, dependency approvals, historical fixtures, or unrelated worktree files. No installs, dependency upgrades, remote changes, or push are part of this task.
  </action>
  <verify>
    <automated>git diff --check &amp;&amp; npm run build:web</automated>
    <human-check>Read README once from a fresh-clone reader's perspective. Check each local Markdown destination exists and every documented npm run name is a key in package.json scripts; distinguish npm ci as a package-manager command. Confirm no completed feature is inferred solely from a planned architecture or schema type. If the prepared local build prerequisite is absent, record the actual failure rather than editing approvals or claiming build success.</human-check>
  </verify>
  <done>README.md exists with accurate purpose, architecture, implemented/planned status, directory map, exact prerequisites, existing commands, and prominent fresh-clone limitations; links and command names are checked, and the local build outcome is recorded honestly.</done>
</task>
</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Public documentation to developer shell | Readers may execute documented setup/build commands. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-README-01 | Tampering | Setup and binary provenance instructions | medium | mitigate | Document the existing receipt/hash gate and prepared-environment limitation without proposing bypasses or unverified downloads. |
| T-README-02 | Information disclosure | Public README | low | mitigate | Use repository-relative paths and public project facts; include no credentials, private machine paths, or local document content. |
</threat_model>

<verification>
Documentation scope only: check links and script names against the repository, run git diff --check, and build the existing inspector once with npm run build:web. Do not rerun the complete product suite for a README-only change.
</verification>

<success_criteria>
A public reader can explain FlowPDF's semantic/PDF split, identify what actually works today, and understand what must be provisioned before using the existing local commands. README does not advertise planned features as implemented.
</success_criteria>

<source_audit>
| Source | Item | Coverage |
|--------|------|----------|
| GOAL | User-requested public README description | Task 1 covers purpose, architecture, map, and developer entry points. |
| REQ | FLOW-01, QUAL-08 | Task 1 documents existing foundation behavior and verification; no completion changes. |
| RESEARCH | Repository manifests, verifier, build/server scripts | Task 1 preserves exact versions, current commands, receipt/hash checks, and ignored artifacts. No new dependencies or external API research needed. |
| CONTEXT | PROJECT.md architecture and user-approved autonomous continuation | Task 1 separates implemented behavior from planned product capabilities. Other implementation plans remain untouched. |
</source_audit>

<output>
After execution, create .planning/quick/260909-mhv-document-flowpdf-purpose-architecture-im/260909-mhv-SUMMARY.md with the documentation changes and actual verification outcome. Parent orchestrator owns shared state and continuation.
</output>
