# Risks

## Redaction leaves recoverable source content
severity: high
mitigation: Treat marking and applying as separate states; require complete rewrite plus post-save text, object, and raster validation; refuse output when any required validator is unavailable or inconclusive.

## Rewriting loses unsupported or unreachable-looking objects
severity: high
mitigation: Build an explicit reachability/preservation model, retain the immutable source, and refuse edits whenever the rewrite cannot prove retention of relevant content.

## Text replacement changes appearance or neighboring content
severity: high
mitigation: Limit editing to recognized text islands with proven font/mapping/geometry support; require layout and clipping checks and return a stable actionable refusal otherwise.

## Page operations detach annotations, widgets, or resources
severity: high
mitigation: Make page graph transformations transactional and verify page, annotation, form, resource, and destination references after serialization.

## Validation falsely implies broad compatibility
severity: medium
mitigation: Keep local parser/writer checks distinct from independent qpdf/Poppler/viewer and external accessibility evidence; report unavailable rows explicitly.
