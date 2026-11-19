import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { isDeepStrictEqual } from "node:util";
import { inflateSync } from "node:zlib";

const root = new URL("../../", import.meta.url);
const corpus = "fixtures/pdf-native/";
const generatorPath = "scripts/generate-pdf-native-fixtures.mjs";
const bytes = (path) => readFileSync(new URL(path, root));
const json = (path) => JSON.parse(bytes(path));
const manifest = json(corpus + "manifest.json");
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const same = (actual, expected, identity) => {
  assert.ok(isDeepStrictEqual(actual, expected), identity);
};
const keys = (value, expected, identity) => {
  same(Object.keys(value).sort(), [...expected].sort(), identity);
};
const bind = (record, path) => {
  keys(record, ["path", "sha256"], path);
  assert.equal(record.path, path);
  assert.ok(/^[a-f0-9]{64}$/.test(record.sha256), path);
  assert.ok(sha256(bytes(path)) === record.sha256, path + " digest mismatch");
};

function verifyEmbeddedFont(pdf, font, id) {
  const headerPattern = new RegExp(`/Filter /FlateDecode /Length (\\d+) /Length1 ${font.length} >>\nstream\n`);
  const header = headerPattern.exec(pdf.toString("latin1"));
  assert.ok(header, id + " font stream missing");
  const start = header.index + header[0].length;
  const end = start + Number(header[1]);
  const decoded = inflateSync(pdf.subarray(start, end), { maxOutputLength: font.length });
  assert.ok(decoded.equals(font), id + " embedded font mismatch");
  const streamEnd = Buffer.from("\nendstream\nendobj\n");
  assert.ok(pdf.subarray(end, end + streamEnd.length).equals(streamEnd), id + " font stream boundary");
}

const matrix = {
  "notes-preservation": {
    profile: "classic-xref-preservation",
    admission: "admitted",
    operations: { preserve: [], addNote: [1], editNote: [1], moveNote: [1, 2], removeNote: [2] },
    text: ["A", "B"],
  },
  refused: {
    profile: "catalog-open-destination",
    admission: "refused",
    operations: { addNote: [] },
    text: ["A"],
  },
};

test("corpus has closed provenance, exact source and expected hashes, and unique fixture IDs", () => {
  keys(manifest, [
    "schemaVersion", "generator", "font", "fixtures", "unsupportedPolicy", "evidence",
  ], "manifest");
  assert.equal(manifest.schemaVersion, 1);
  same(manifest.generator, { path: generatorPath, version: 1 });
  keys(manifest.font, ["binary", "license", "spdx"], "font");
  assert.equal(manifest.font.spdx, "OFL-1.1");
  bind(manifest.font.binary, "crates/flow-core/data/NotoSans-Regular.ttf");
  bind(manifest.font.license, "crates/flow-core/data/NotoSans-OFL.txt");
  assert.equal(manifest.font.binary.sha256,
    "b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5");
  assert.equal(manifest.font.license.sha256,
    "12e55f73bf39d88d7f2e37e094ad30b8f6752b3d25391ac23978b1e5e35ca66b");

  same(manifest.fixtures.map(({ id }) => id), Object.keys(matrix));
  const font = bytes(manifest.font.binary.path);
  for (const fixture of manifest.fixtures) {
    const { id, provenance, profile, admission, source, expected } = fixture;
    keys(fixture, ["id", "provenance", "profile", "admission", "source", "expected"], id);
    same(provenance, {
      origin: "original-synthetic", license: "CC0-1.0", generatorVersion: 1,
    }, id);
    assert.equal(profile, matrix[id].profile, id);
    assert.equal(admission, matrix[id].admission, id);
    bind(source, corpus + id + ".pdf");
    verifyEmbeddedFont(bytes(source.path), font, id);
    if (id === "notes-preservation") {
      bind(expected, corpus + id + ".expected.json");
    } else {
      keys(expected, ["inline", "sha256"], id);
      assert.ok(sha256(JSON.stringify(expected.inline)) === expected.sha256, id + " expectation digest mismatch");
    }
  }
});

test("every operation has complete page text, retention and versioned raster expectations", () => {
  const retained = [
    "catalog-custom-scalars", "trailer-info-and-id", "shared-resources",
    "indirect-length", "encoded-content-streams", "embedded-noto-sans",
  ];
  for (const { id, expected } of manifest.fixtures) {
    const { operations, text, admission } = matrix[id];
    const data = expected.inline ?? json(expected.path);
    keys(data, ["schemaVersion", "fixtureId", "raster", "operations"], id);
    assert.equal(data.schemaVersion, 1, id);
    assert.equal(data.fixtureId, id);
    same(data.raster, {
      policy: "page-content-rgb8", version: 1, dpi: 144, origin: "top-left",
      annotationLayer: "excluded", evidence: "designed",
    }, id);
    same(data.operations.map(({ operation }) => operation), Object.keys(operations), id);
    for (const entry of data.operations) {
      const label = id + "/" + entry.operation;
      const affectedPages = operations[entry.operation];
      const refused = admission === "refused";
      keys(entry, [
        "operation", "outcome", "affectedPages", "noteReadback", "retained", "removed", "pages",
      ], label);
      same(entry.outcome, refused ? "FLOW_PDF_NATIVE_UNSUPPORTED" : "admitted", label);
      same(entry.affectedPages, affectedPages, label);
      same(entry.retained, refused ? ["original-source"] : [
        ...retained, ...(entry.operation === "preserve" ? ["original-source", "UnreachableDecoy"] : []),
      ], label);
      same(entry.removed, refused || entry.operation === "preserve" ? [] : [
        "UnreachableDecoy", ...(entry.operation === "editNote" ? ["Review note"] : []),
        ...(entry.operation === "removeNote" ? ["Moved note"] : []),
      ], label);
      same(entry.noteReadback, {
        addNote: refused ? [] : [{ page: 1, rectPt: [96, 72, 112, 88], contents: "Review note" }],
        editNote: [{ page: 1, rectPt: [96, 72, 112, 88], contents: "Moved note" }],
        moveNote: [{ page: 2, rectPt: [120, 56, 136, 72], contents: "Moved note" }],
      }[entry.operation] ?? [], label);
      same(entry.pages, text.map((value, index) => ({
        page: index + 1, text: value, cropBoxPt: [0, 0, 160, 100], rotation: 0,
        widthPx: 320, heightPx: 200,
        comparison: refused ? "source-only" : "equal-to-source",
        maxDifferentPixels: refused ? null : 0,
      })), label);
    }
  }
});

test("policy-only categories and unavailable observations grant no candidate authority", () => {
  same(manifest.unsupportedPolicy, [
    "encryption", "incremental-updates", "hybrid-xref", "object-and-xref-streams",
    "signatures-and-permissions", "active-actions", "embedded-files-and-media", "xfa",
  ]);
  same(manifest.evidence, {
    fixtureExpectations: "designed",
    localVerifier: { status: "not-run", independence: "shared-code" },
    externalTools: { status: "unavailable", observations: [] },
    candidateAuthorization: "none",
  });
});

test("generator is independent of the application and native writer", () => {
  const source = bytes(generatorPath).toString("utf8");
  const imports = [...source.matchAll(/^import .+ from "([^"]+)";$/gm)].map((match) => match[1]);
  same(imports, ["node:crypto", "node:fs", "node:zlib"], "generator imports");
  assert.equal([...source.matchAll(/\bimport\b(?!\.meta)/g)].length, imports.length);
  assert.equal(/\b(?:import\s*\(|require\s*\(|eval\s*\()/.test(source), false);
  assert.ok(source.includes("startxref") && source.includes("endobj"), "literal classic xref");
});

const wasmModule = new URL("web/generated/flow_wasm.js", root);
const wasmBinary = new URL("web/generated/flow_wasm_bg.wasm", root);
const missingWasm = !existsSync(wasmModule) || !existsSync(wasmBinary);

test("the preservation fixture completes its note lifecycle with built WASM", {
  skip: missingWasm && "Build the production WASM module to run the fixture lifecycle",
}, async () => {
  const wasm = await import(wasmModule.href);
  wasm.initSync({ module: readFileSync(wasmBinary) });
  const input = bytes(corpus + "notes-preservation.pdf");
  const originStoreHash = sha256(input);
  let requestId = 0;
  const exchange = (handler, operation, payload) => {
    const response = handler(JSON.stringify({
      protocolVersion: 1, requestId: ++requestId, originStoreHash, operation, ...payload,
    }));
    assert.ok(wasm.verify_native_pdf_response(response), operation + " verification");
    return JSON.parse(response);
  };
  const opened = exchange(wasm.native_pdf_open, "open", {
    epoch: Array(16).fill(1), bytesHex: input.toString("hex"),
  });
  assert.equal(opened.status, "opened");
  assert.equal(opened.snapshot.outputHex, input.toString("hex"));
  let snapshot = opened.snapshot;
  let note;
  const commands = {
    addNote: ({ page, rectPt, contents }) => ({ kind: "add", page: page - 1, rect: rectPt, contents }),
    editNote: ({ contents }) => ({ kind: "edit", note, contents }),
    moveNote: ({ page, rectPt }) => ({ kind: "move", note, page: page - 1, rect: rectPt }),
    removeNote: () => ({ kind: "remove", note }),
  };
  const expectations = json(corpus + "notes-preservation.expected.json");
  for (const { operation, noteReadback } of expectations.operations.slice(1)) {
    const { binding, outputHash } = snapshot;
    const result = exchange(wasm.native_pdf_command, "command", {
      binding, outputHash, command: commands[operation](noteReadback[0]),
    });
    assert.equal(result.status, "applied", operation + " " + result.code);
    snapshot = result.snapshot;
    assert.equal(snapshot.binding.revision, binding.revision + 1, operation);
    const notes = snapshot.scene.pages.flatMap((page) => page.annotations
      .filter(({ kind }) => kind === "plainNote")
      .map(({ id, rect, contents }) => ({ id, page: page.id + 1, rectPt: rect, contents })));
    assert.deepEqual(notes.map(({ page, rectPt, contents }) => ({ page, rectPt, contents })), noteReadback);
    note = notes[0]?.id;
  }
  assert.equal(exchange(wasm.native_pdf_close, "close", { binding: snapshot.binding }).status, "closed");
});
