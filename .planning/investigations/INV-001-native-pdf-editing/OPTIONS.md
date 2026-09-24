# Options

<!-- Draft options; no recommendation is selected during research. -->

## Option A — Rewrite a bounded source object graph

Use the owned Rust reader and COS writer to retain supported and safely opaque source objects, apply typed page/object edits, then emit a fresh complete PDF. Refuse a change if the affected graph cannot be retained and validated.

## Option B — Rebuild each edited page from the recognized scene

Render supported scene elements into newly authored page content and assemble a new PDF. Unsupported content would need a separately proven preservation representation or the affected edit would be refused.

## Option C — Keep native PDFs read-only

Defer native editing and redaction. Continue supporting reader/reconstruction while maintaining imported bytes as immutable source records.

## Comparison

| | Option A | Option B | Option C |
|---|---|---|---|
| Complexity | High: object graph retention and deterministic rewriting | High: scene-to-content emission, resource rebuilding, and fidelity limits | Low |
| Risks | Incorrect reachability or serialization may lose hidden or interactive content | Flattening/rebuilding can change text selection, forms, links, accessibility, and appearance | Does not deliver NPDF-01 through NPDF-05 |
| What it forecloses | Fast implementation without a preservation proof; edits to unrecognized affected objects | Reliable preservation of source semantics and per-object provenance | Native corrections, page operations, and redaction |
