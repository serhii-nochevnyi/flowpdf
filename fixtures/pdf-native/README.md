# Native PDF fixture corpus

The original synthetic PDF syntax and authored expectations are dedicated to the public domain under CC0-1.0. Both PDFs embed the repository's unmodified Noto Sans Regular binary under OFL-1.1. The manifest pins the binary and its full license text by SHA-256.

## Reproduce

From the repository root with the pinned Node runtime, run:

```sh
node scripts/generate-pdf-native-fixtures.mjs
node scripts/generate-pdf-native-fixtures.mjs --check
node --test tests/contracts/pdf-native-fixtures.test.mjs
```

The generator uses literal PDF objects, explicit generations, byte offsets and classic xref entries. It reads only the pinned font and license as fixture inputs. It uses fixed-strategy Flate for the two short page streams. It has no application, WASM or production writer dependency. Generation writes the two PDFs. The manifest and expectation data are authored separately. The manifest binds source and sidecar files by exact byte digest. The inline refusal expectation is hashed as UTF-8 JSON serialized with `JSON.stringify`.

## Profiles and expectations

- **Preservation and notes** has two pages with distinct selectable text and one shared resource dictionary. The preservation baseline is an open with no edits, so the output stays byte-identical to the source. Then add a note on page 1, edit its contents, move it to page 2, and remove it. Page indices in the expectations are one-based. Note rectangles use PDF points, with the origin at the bottom left.
- **Refused source** uses a catalog `/OpenAction` whose value is an explicit local page destination array. It is an ordinary viewable document. The native edit admission policy refuses that catalog key before any note edit or candidate output.
- The manifest's unsupported policy list records categories outside the edit profile. Those categories have no observation or fixture coverage in this corpus.

The preservation identifiers refer to the catalog's `[true false +.125 /Review#20Flag]`, the trailer's referenced `/Info` dictionary and `/ID` pair, the shared `/Resources` reference, the first content stream's indirect `/Length`, the exact encoded bytes of both Flate content streams, and the unmodified embedded font and text mapping. Rewrites may renumber objects. The unreachable `/UnreachableDecoy` object is present in the source and is expected to disappear on the first rewrite.

Raster policy v1 compares opaque RGB8 page content at 144 dpi, starting from the top-left of the effective crop box. Compare each edited page to the same source page at the declared dimensions, with zero differing pixels. This also applies to pages untouched by the operation. The comparison excludes viewer-specific note icons and popups. Note appearance-independent readback is recorded as the annotation page, rectangle and contents. The refused source declares page dimensions for inspection only and has no candidate raster comparison.

## Evidence limits

The contracts verify fixture identity, asset provenance and completeness of designed expectations. A separate local verifier can share parsing and font code with the application, so its results are shared-code evidence. The corpus records no local verifier run, and external evidence remains unavailable until a real run records the tool, version, scoped inputs and outputs. The current coverage is preservation, notes and refusal. Later fixtures can extend the same schema with the remaining edit families. Each user candidate requires its own verification. Corpus results grant no authorization to other candidates.
