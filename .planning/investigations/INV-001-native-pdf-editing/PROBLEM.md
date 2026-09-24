---
status: open          # open | closed — Gate 1 sets `closed`
closed:               # YYYY-MM-DD, filled in at Gate 1
adr:                  # path to .planning/architecture/ADR-NNN-*.md, filled in at Gate 1
---

# Problem

## What we are solving

Define a safe, bounded way to edit supported content in imported PDF files while preserving their fixed-page character. The investigation covers annotations and AcroForm widgets, recognized text islands, supported images and simple graphics, page operations, and permanent redaction through a complete rewrite.

## For whom

Authors and reviewers working with Ukrainian- and English-language contracts, forms, reports, and similar professional documents who need to correct or manage a PDF without converting the whole file into a reflowing FlowDocument.

## Current pain

The PDF reader currently exposes a read-only scene, while reconstruction produces a separate best-effort FlowDocument candidate. Neither path edits the imported PDF in its native page model. The owned PDF writer emits FlowPDF documents from semantic inputs; it is not yet a rewrite path for imported object graphs.

## What success will be

The Phase 9 design enables safe edits to the declared supported subset, retains unsupported content or refuses an unsafe edit, surfaces actionable warnings for unsupported fonts/transforms/clipping/resources, supports the declared page operations, and applies redaction only after a complete rewrite passes post-save text, object, and visual checks. The original import remains an immutable source and edited output is a derived file.

## What is definitely out of scope

- Arbitrary-PDF editing or silently repairing unsupported syntax.
- Executing PDF actions, following external resources, or accepting unbounded input.
- Editing through FlowDocument reconstruction or changing semantic-document transactions.
- Encryption, decryption, signature creation, or preserving an existing digital signature after modification, consistent with the Phase 7 security boundary.
- Claiming secure redaction when content removal or post-save verification is incomplete.
