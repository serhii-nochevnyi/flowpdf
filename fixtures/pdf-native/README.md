# Native PDF fixture corpus

The original synthetic PDF syntax and authored expectations are dedicated to the public domain under CC0-1.0. All four PDFs embed the repository's unmodified Noto Sans Regular binary under OFL-1.1. The manifest pins the binary and its full license text by SHA-256.

## Reproduce

From the repository root with the pinned Node runtime, run:

```sh
node scripts/generate-pdf-native-fixtures.mjs
node scripts/generate-pdf-native-fixtures.mjs --check
node --test tests/contracts/pdf-native-fixtures.test.mjs
```

The generator uses literal PDF objects, explicit generations, byte offsets and classic xref entries. It reads only the pinned font and license as fixture inputs. It uses run-length Flate for the font and fixed-strategy Flate for the preservation streams and grayscale samples. It can regenerate all four PDFs from those inputs. It has no application, WASM or production writer dependency. The manifest and expectation data are authored separately. The manifest binds source and sidecar files by exact byte digest. The inline refusal expectation is hashed as UTF-8 JSON serialized with `JSON.stringify`.

## Profiles and expectations

- **Preservation and notes** has two pages with distinct selectable text and one shared resource dictionary. The preservation baseline is an open with no edits, so the output stays byte-identical to the source. Then add a note on page 1, edit its contents, move it to page 2, and remove it. Page indices in the expectations are one-based. Note rectangles use PDF points, with the origin at the bottom left.
- **Refused source** uses a catalog `/OpenAction` whose value is an explicit local page destination array. It is an ordinary viewable document. The native edit admission policy refuses that catalog key before any note edit or candidate output.
- **Edit families** has reversible Identity-H text using the embedded A, B and Ї glyphs. The first page owns its text stream, a Text note and text/checkbox widgets with explicit appearances. The second and third pages share a graphics stream that paints a rectangle and invokes the same gray image twice. The second leaf inherits a cropped, rotated view from its parent. Its link and the document outline target the first leaf. Object edits target the third leaf, so the second leaf and the unedited image invocation test isolation. Operations cover fitting replacement, graphics and image creation/replacement, movement and removal, page insertion/reordering/rotation/duplication/deletion, and text/checkbox field editing. Refusals cover unmapped text, overflow, skew, final-page deletion, unmapped field glyphs and attempting a scrub on this form-bearing document.
- **Redaction** is a form-free scrub profile with marked text, a rectangle, one sample of a two-sample image, and a Text note. The complete rewrite removes document info and identity, catalog/page metadata, and the private source envelope. The unmarked gray sample, blue rectangle, control note, and second page are retained. The second page has selectable text and a green rectangle.
- The manifest records additional unsupported categories as policy only. The corpus exercises the explicit catalog destination refusal and operation refusals described here.

The preservation identifiers refer to the catalog's `[true false +.125 /Review#20Flag]`, the trailer's referenced `/Info` dictionary and `/ID` pair, the shared `/Resources` reference, the first content stream's indirect `/Length`, the exact encoded bytes of both Flate content streams, and the unmodified embedded font and text mapping. Rewrites may renumber objects. The unreachable `/UnreachableDecoy` object is present in the source and is expected to disappear on the first rewrite.

Raster policy v1 compares opaque RGB8 page content at 144 dpi, starting from the top-left of the effective crop box. Compare each edited page to the same source page at the declared dimensions, with zero differing pixels. This also applies to pages untouched by the operation. The comparison excludes viewer-specific note icons and popups. Note appearance-independent readback is recorded as the annotation page, rectangle and contents. The refused source declares page dimensions for inspection only and has no candidate raster comparison.

## Edit-family and redaction comparisons

Schema v2 starts each operation from its fixture source, with any prerequisite steps listed in the request. Requests use one-based page numbers and positions, and unrotated PDF points. Convert these to the command's native page identifiers, indices and units when applying them. Resolve text and object targets by their source placement and field targets by name. The hexadecimal PNG requests carry their own one-pixel input. Text expectations join content runs in source stream order with an ASCII space. The note, field and destination readback records cover annotations separately from page text and raster. Compare readback by page and identity, independent of object numbering and record order.

Each output page declares which source page supplies its baseline. Affected source pages also account for removed leaves and changed inbound destinations. Raster comparisons use opaque RGB8 sampled at 144 dpi, with the top-left of the effective crop as the origin and the declared clockwise rotation applied. The RGB bytes are tightly packed in row-major order. The annotation layer, including widget appearances, is excluded from page raster. Inspect field values, default values and checkbox appearance state through the readback records.

- `equal-to-source` compares every pixel exactly against the corresponding source page using the same renderer. `source-transformed` rotates the source raster by the declared rotation difference and compares exactly.
- `source-with-patches` compares the entire page against the source with the specified PDF-point patches. Sample aligned flat fills at pixel centers. The text patch clears the area to white and draws the listed CIDs in black from the pinned font, using the declared baseline, size, and authored CID widths. The one-pixel band around authored glyph contours allows an absolute per-channel difference of at most 64. All other pixels compare exactly, including flat-color patches and objects outside the target rectangles.
- `analytic-full-page` starts white and applies the specified fills. The blank inserted page remains white. On redaction page 1, the removed text and vector areas return to white. Any marked image sample with positive-area intersection is zeroed across its full projected cell. The other sample stays at gray 83 and the control rectangle stays blue. The aligned fills cover all remaining nonwhite pixels. These baselines have zero channel tolerance across the full raster and pin the exact RGB8 buffer digest.

A refused operation preserves its operation base bytes. For final-page deletion the operation base is the result of deleting the first and third source leaves in that order. The remaining leaf still uses its original source-page raster. No candidate is expected from refused operations.

## Evidence limits

The contracts verify fixture identity, the decoded font bytes, and completeness of designed expectations. With a built production WASM module, they also run the note lifecycle. A separate local verifier can share parsing and font code with the application, so its results are shared-code evidence. The corpus records no local verifier run, and external evidence remains unavailable until a real run records the tool, version, scoped inputs and outputs. Designed expectations describe only the checked-in sources and declared operations. Each user candidate requires its own scoped verification.
