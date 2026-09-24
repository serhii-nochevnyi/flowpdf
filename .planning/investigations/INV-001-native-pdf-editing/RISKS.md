# Risks

## Redaction leaves recoverable source content
severity: high
mitigation: Keep marking and applying as separate states; remove content in streams/resources and rewrite the complete reachable graph into new bytes; strip metadata; verify text, object reachability, and raster output; publish no redacted output when a required validator is unavailable or inconclusive. Require a human checkpoint before the redaction implementation ticket.

## Rewriting loses unsupported or unreachable-looking objects
severity: high
mitigation: Build a bounded source graph before mutation, preserve parsed generic reachable COS values and untouched raw streams, keep the original immutable, and refuse the complete edit on parse/retention uncertainty. Drop only objects unreachable from the admitted root after validating graph references.

## Text replacement changes appearance or neighboring content
severity: high
mitigation: Admit only reversible encodings, supported embedded fonts, simple single-line text-show operators, and supported geometry; reject overflow and unsupported clipping/transforms with stable diagnostics. Keep visual before/after fixtures.

## Page operations detach annotations, widgets, or resources
severity: high
mitigation: Make page graph transformations transactional and verify page, annotation, form, resource, and destination references after serialization.

## A rewrite retains active or external behavior unintentionally
severity: high
mitigation: Never execute or follow actions/resources; preflight the reachable graph for encryption, signatures, JavaScript, launch actions, external references, XFA, and embedded files. Preserve only parseable passive generic values for ordinary edits; refuse redaction where the complete content surface cannot be inspected.

## Browser publishes stale or cross-source edits
severity: high
mitigation: Bind each session and command to source hash, session revision, and request identity; publish only Rust-verified worker responses and retain the immutable imported source.

## Validation falsely implies broad compatibility
severity: high
mitigation: Separate local round-trip/property tests from qpdf structure, Poppler text/raster, and target-viewer checks; report each unavailable row and never infer arbitrary-PDF compatibility from controlled fixtures.
