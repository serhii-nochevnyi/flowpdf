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

const expanded = {
  "edit-families": {
    profile: "embedded-text-shared-graphics-pages-fields",
    operations: {
      replaceText: "", replaceTextAbsentCode: "TEXT_MAPPING", replaceTextOverflow: "TEXT_OVERFLOW",
      addGraphic: "", replaceGraphic: "", addImage: "", replaceImage: "",
      moveObject: "", removeObject: "", moveObjectInvalidGeometry: "OBJECT_GEOMETRY",
      insertBlankPage: "", reorderPage: "", rotatePage: "", duplicatePage: "", deletePage: "",
      deleteFinalPage: "FINAL_PAGE",
      addTextField: "", addCheckboxField: "", setFieldText: "", setFieldChecked: "",
      moveField: "", removeField: "", setFieldTextAbsentGlyph: "FIELD_GLYPH",
      redactWithForms: "UNSUPPORTED",
    },
  },
  redaction: {
    profile: "scrub-text-path-image-note-metadata",
    operations: { redact: "" },
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

  same(manifest.fixtures.map(({ id }) => id), [...Object.keys(matrix), ...Object.keys(expanded)]);
  const font = bytes(manifest.font.binary.path);
  for (const fixture of manifest.fixtures) {
    const { id, provenance, profile, admission, source, expected } = fixture;
    keys(fixture, ["id", "provenance", "profile", "admission", "source", "expected"], id);
    same(provenance, {
      origin: "original-synthetic", license: "CC0-1.0", generatorVersion: 1,
    }, id);
    assert.equal(profile, (matrix[id] ?? expanded[id]).profile, id);
    assert.equal(admission, matrix[id]?.admission ?? "admitted", id);
    bind(source, corpus + id + ".pdf");
    verifyEmbeddedFont(bytes(source.path), font, id);
    if (id !== "refused") {
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
    if (expanded[id]) continue;
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

const crop = [0, 0, 160, 100];
const rotatedCrop = [4, 4, 156, 96];
const pagePlans = {
  replaceText: [[1], [1, 2, 3], "source-with-patches"],
  addGraphic: [[3], [1, 2, 3], "source-with-patches"],
  replaceGraphic: [[3], [1, 2, 3], "source-with-patches"],
  addImage: [[3], [1, 2, 3], "source-with-patches"],
  replaceImage: [[3], [1, 2, 3], "source-with-patches"],
  moveObject: [[3], [1, 2, 3], "source-with-patches"],
  removeObject: [[3], [1, 2, 3], "source-with-patches"],
  insertBlankPage: [[], [1, 2, null, 3]],
  reorderPage: [[1, 2], [2, 1, 3]],
  rotatePage: [[2], [1, 2, 3]],
  duplicatePage: [[1], [1, 1, 2, 3]],
  deletePage: [[1, 2], [2, 3]],
  deleteFinalPage: [[], [2]],
  addTextField: [[1], [1, 2, 3]],
  addCheckboxField: [[1], [1, 2, 3]],
  setFieldText: [[1], [1, 2, 3]],
  setFieldChecked: [[1], [1, 2, 3]],
  moveField: [[1], [1, 2, 3]],
  removeField: [[1], [1, 2, 3]],
  redact: [[1], [1, 2], "analytic-full-page"],
};

const white = [255, 255, 255];
const green = [0, 255, 0];
const newGray = [64, 64, 64];
const originalGray = [160, 160, 160];
const pathRect = [16, 44, 28, 56];
const firstImageRect = [48, 44, 64, 56];
const newObjectRect = [106, 44, 122, 56];
const patch = (rectPt, rgb) => ({ rectPt, rgb });
const pagePatches = {
  replaceText: [{
    ...patch([12, 70, 40, 92], white),
    glyphRun: { cids: [3, 1], baselinePt: [16, 76], sizePt: 12 },
  }],
  addGraphic: [patch(newObjectRect, green)],
  replaceGraphic: [patch(pathRect, green)],
  addImage: [patch(newObjectRect, newGray)],
  replaceImage: [patch(firstImageRect, newGray)],
  moveObject: [patch(firstImageRect, white), patch([48, 32, 64, 44], originalGray)],
  removeObject: [patch(pathRect, white)],
};

function verifyEditReadback(entry, order, label) {
  const operation = entry.operation;
  const noteRect = [128, 68, 144, 84];
  const textRect = [16, 12, 64, 32];
  const checkboxRect = [80, 12, 100, 32];
  const textField = (page, name, rectPt, value = "", defaultValue = "") =>
    ({ page, rectPt, name, value, defaultValue });
  const checkbox = (page, name, rectPt, value = "Off") =>
    ({ page, rectPt, name, value, defaultValue: "Off", appearanceState: value });

  const textPages = order.flatMap((source, index) => source === 1 ? [index + 1] : []);
  same(entry.noteReadback, textPages.map((page) =>
    ({ page, rectPt: noteRect, contents: "Control note" })), label + " notes");

  const fields = textPages.flatMap((page, index) => {
    if (index > 0) return [
      textField(page, "FlowPDFCopy18_0", textRect),
      checkbox(page, "FlowPDFCopy19_0", checkboxRect),
    ];

    const result = [];
    if (operation !== "removeField") {
      result.push(textField(page, "recipient", textRect, operation === "setFieldText" ? "ЇA" : ""));
    }
    result.push(checkbox(page, "accepted", operation === "moveField" ? [104, 12, 124, 32] : checkboxRect,
      operation === "setFieldChecked" ? "Yes" : "Off"));
    if (operation === "addTextField") {
      result.push(textField(page, "reviewer", [104, 12, 152, 32], "AB", "AB"));
    }
    if (operation === "addCheckboxField") {
      result.push(checkbox(page, "approval", [104, 12, 124, 32]));
    }
    return result;
  });
  same(entry.fieldReadback, fields, label + " fields");

  const [target] = textPages;
  same(entry.destinations, target === undefined ? [] : [
    { kind: "outline", title: "First page", target },
    { kind: "link", page: order.indexOf(2) + 1, target },
  ], label + " destinations");
}

function verifyPaint(paint, bounds, label, allowGlyphs) {
  keys(paint, ["rectPt", "rgb", ...(allowGlyphs ? ["glyphRun"] : [])], label);
  const [left, bottom, right, top] = paint.rectPt;
  assert.ok(paint.rectPt.length === 4 && paint.rectPt.every(Number.isInteger)
    && left >= bounds[0] && bottom >= bounds[1] && right <= bounds[2]
    && top <= bounds[3] && left < right && bottom < top, label + " paint bounds");
  assert.ok(paint.rgb.length === 3
    && paint.rgb.every((n) => Number.isInteger(n) && n >= 0 && n <= 255), label + " paint color");
  if (allowGlyphs) {
    same(paint.glyphRun, { cids: [3, 1], baselinePt: [16, 76], sizePt: 12 }, label);
  }
}

function verifyAnalyticRaster(page, label, expectedFills) {
  keys(page.raster, ["comparison", "fills", "sha256Rgb8"], label);
  same(page.raster.fills, expectedFills, label);
  const rgb = Buffer.alloc(page.widthPx * page.heightPx * 3, 255);
  for (const fill of page.raster.fills) {
    verifyPaint(fill, page.cropBoxPt, label, false);
    // Full-page colored baselines are unrotated. The rotated inserted page is white.
    const [left, bottom, right, top] = fill.rectPt;
    for (let row = page.heightPx - 2 * top; row < page.heightPx - 2 * bottom; row++) {
      const start = (row * page.widthPx + 2 * left) * 3;
      rgb.fill(Buffer.from(fill.rgb), start, start + (right - left) * 6);
    }
  }
  assert.ok(sha256(rgb) === page.raster.sha256Rgb8, label + " analytic raster digest");
}

test("edit families and redaction pin operation outcomes, page identities, text and full raster rules", () => {
  const fills = [
    { rectPt: [48, 44, 64, 56], rgb: [0, 0, 0] },
    { rectPt: [64, 44, 80, 56], rgb: [83, 83, 83] },
    { rectPt: [112, 20, 128, 32], rgb: [0, 0, 255] },
  ];
  for (const [id, profile] of Object.entries(expanded)) {
    const data = json(corpus + id + ".expected.json");
    keys(data, ["schemaVersion", "fixtureId", "raster", "operations"], id);
    same(data.schemaVersion, 2, id);
    same(data.fixtureId, id, id);
    same(data.raster, {
      policy: "page-content-rgb8", version: 2, dpi: 144, origin: "top-left",
      annotationLayer: "excluded", evidence: "designed", backgroundRgb: [255, 255, 255],
      pixelCoverage: "positive-area image sample intersection; pixel centers for aligned flat fills",
      flatChannelTolerance: 0, edgeBandPx: 1, edgeChannelTolerance: 64,
    }, id);
    same(data.operations.map(({ operation }) => operation), Object.keys(profile.operations), id);
    for (const entry of data.operations) {
      const op = entry.operation;
      const label = id + "/" + op;
      keys(entry, [
        "operation", "request", "outcome", "affectedSourcePages", "affectedPages", "output",
        "removed", "noteReadback", "fieldReadback", "destinations", "pages",
      ], label);
      assert.ok(Object.keys(entry.request).length > 0, label + " request missing");
      const refusal = profile.operations[op];
      const [affected, order, firstComparison] = pagePlans[op] ?? [[], [1, 2, 3]];
      same(entry.outcome, refusal ? "FLOW_PDF_NATIVE_" + refusal : "admitted", label);
      same(entry.output, refusal ? "byte-identical-to-operation-base" : "rewritten", label);
      same(entry.affectedSourcePages, affected, label);
      same(entry.affectedPages, order.flatMap((source, index) =>
        affected.includes(source) || source === null ? [index + 1] : []), label);
      same(entry.pages.map(({ sourcePage }) => sourcePage), order, label);
      if (id === "edit-families") verifyEditReadback(entry, order, label);
      for (const [index, page] of entry.pages.entries()) {
        const pageLabel = label + "/page-" + (index + 1);
        keys(page, ["page", "sourcePage", "text", "cropBoxPt", "rotation", "widthPx", "heightPx", "raster"], pageLabel);
        same(page.page, index + 1, pageLabel);
        const inherited = id === "edit-families"
          && (page.sourcePage === 2 || page.sourcePage === null);
        const rotation = inherited ? (op === "rotatePage" ? 180 : 90) : 0;
        same(page.rotation, rotation, pageLabel);
        same(page.cropBoxPt, inherited ? rotatedCrop : crop, pageLabel);
        same([page.widthPx, page.heightPx], inherited
          ? (rotation === 90 ? [184, 304] : [304, 184])
          : [320, 200], pageLabel);
        const text = id === "redaction" ? (index === 0 ? "" : "B")
          : page.sourcePage !== 1 ? "" : op === "replaceText" ? "ЇA B" : "ЇAA B";
        same(page.text, text, pageLabel);
        let comparison = affected.includes(page.sourcePage) && firstComparison
          ? firstComparison : "equal-to-source";
        if (page.sourcePage === null) comparison = "analytic-full-page";
        if (op === "rotatePage" && page.sourcePage === 2) comparison = "source-transformed";
        same(page.raster.comparison, comparison, pageLabel);
        if (comparison === "analytic-full-page") {
          verifyAnalyticRaster(page, pageLabel, id === "redaction" ? fills : []);
        } else if (comparison === "source-with-patches") {
          keys(page.raster, ["comparison", "patches"], pageLabel);
          same(page.raster.patches, pagePatches[op], pageLabel);
          for (const paint of page.raster.patches) verifyPaint(paint, crop, pageLabel, op === "replaceText");
        } else {
          keys(page.raster, ["comparison"], pageLabel);
        }
      }
    }
  }
});

test("redaction accounts for sensitive families and preserves control content", () => {
  const [entry] = json(corpus + "redaction.expected.json").operations;
  same(entry.request.marks, [
    { page: 1, rectPt: [12, 70, 40, 92] },
    { page: 1, rectPt: [16, 44, 28, 56] },
    { page: 1, rectPt: [51, 46, 52, 47] },
    { page: 1, rectPt: [96, 68, 112, 84] },
  ], "redaction marks");
  same(entry.removed, [
    "page-1 text ЇAA", "red vector fill", "image sample 217", "Sensitive note",
    "Sensitive title", "Sensitive author", "Sensitive catalog metadata",
    "Sensitive page metadata", "Sensitive source envelope", "trailer ID",
  ], "removed families");
  same(entry.noteReadback, [
    { page: 1, rectPt: [128, 68, 144, 84], contents: "Control note" },
  ], "retained annotation");
  same(entry.fieldReadback, [], "forms excluded from positive scrub");
  same(entry.destinations, [], "redaction has no destinations");
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
