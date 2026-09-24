# Resolved questions

- [x] First end-to-end tracer: add, move, edit, and remove a plain `/Text`
  note in one Rust-owned session and fresh rewrite, exercised through Rust,
  string-only WASM, a worker, and the native-PDF browser surface. Use a
  deterministic one-page classic-xref fixture with one supported note and one
  untouched generic resource.
- [x] Unsupported objects: preserve fully parsed reachable generic COS values
  and unmodified streams as bounded raw encoded data. Refuse the complete edit
  before producing output if a reachable object, reference, stream encoding,
  trailer, or page dependency cannot be represented safely. Do not silently
  flatten/rebuild a scene.
- [x] Editable text island: require reversible source character-code mapping,
  supported embedded font, admitted single-line text-show operators, no
  unsupported clipping/transform/rendering mode/resource ambiguity, and an
  encodable replacement that fits the original geometry. Any uncertainty or
  overflow is a typed refusal with an actionable warning.
- [x] Page insertion means blank-page insertion only. Reorder, rotation,
  same-document duplication, and deletion are included; cross-document page
  import is deferred. Deleting the last page is refused.
- [x] Redaction removes intersecting recognized text-show units, whole
  intersecting simple vector objects, destructively modified admitted raster
  pixels, and intersecting supported annotations. It never uses an overlay as
  removal. Refuse input with unrecognized content, unsupported transforms,
  clips/masks, unhandled forms/attachments, signatures, encryption, or
  non-page text/content that cannot be scrubbed. Strip document metadata from
  redacted output; keep the original source immutable.
- [x] Verification uses deterministic local fixtures plus internal post-save
  text/reachability checks, and requires independent qpdf structural,
  Poppler text-extraction, and Poppler raster checks in the release lane.
  `command -v qpdf pdftotext pdftoppm mutool` returned no paths here; absence
  is `unavailable`, never a local pass. qpdf alone does not prove secure
  redaction.
- [x] Interactive scope: plain Text notes and bounded non-signature text and
  checkbox AcroForm fields. Exclude signature widgets, XFA, JavaScript,
  launch/external actions, embedded files, and arbitrary annotation
  appearances. Signature-bearing or encrypted PDFs remain read-only and
  native edit/save is refused with a stable explanation.

These choices apply the user's instruction to proceed autonomously with
recommended technical decisions approved. They do not claim broad PDF
compatibility or remove the high-risk redaction checkpoint in the delivery
plan.
