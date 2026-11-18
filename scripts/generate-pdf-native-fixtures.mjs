import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { constants, deflateSync } from "node:zlib";

const output = new URL("../fixtures/pdf-native/", import.meta.url);
const data = new URL("../crates/flow-core/data/", import.meta.url);
const MAX_BYTES = 1024 * 1024;

function pinnedAsset(name, sha256) {
  const bytes = readFileSync(new URL(name, data));
  if (bytes.length > MAX_BYTES || createHash("sha256").update(bytes).digest("hex") !== sha256) {
    throw new Error("Fixture input mismatch: " + name);
  }
  return bytes;
}

function stream(dictionary, bytes) {
  return Buffer.concat([
    Buffer.from("<< " + dictionary + " >>\nstream\n"),
    Buffer.from(bytes),
    Buffer.from("\nendstream"),
  ]);
}

function classicPdf(objects, trailer) {
  if (objects.length === 0 || objects.length > 64) throw new Error("Fixture object limit");
  const chunks = [Buffer.from("%PDF-1.7\n%\xe2\xe3\xcf\xd3\n", "latin1")];
  let offset = chunks[0].length;
  const entries = ["0000000000 65535 f \n"];
  for (const [index, [generation, body]] of objects.entries()) {
    if (!Number.isInteger(generation) || generation < 0 || generation >= 65535) {
      throw new Error("Fixture generation limit");
    }
    const header = Buffer.from(`${index + 1} ${generation} obj\n`);
    const value = Buffer.from(body);
    const end = Buffer.from("\nendobj\n");
    entries.push(`${String(offset).padStart(10, "0")} ${String(generation).padStart(5, "0")} n \n`);
    chunks.push(header, value, end);
    offset += header.length + value.length + end.length;
    if (offset > MAX_BYTES) throw new Error("Fixture byte limit");
  }
  chunks.push(Buffer.from(
    `xref\n0 ${entries.length}\n${entries.join("")}` +
    `trailer\n<< /Size ${entries.length} ${trailer} >>\nstartxref\n${offset}\n%%EOF\n`,
  ));
  return Buffer.concat(chunks);
}

const font = pinnedAsset("NotoSans-Regular.ttf",
  "b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5");
pinnedAsset("NotoSans-OFL.txt",
  "12e55f73bf39d88d7f2e37e094ad30b8f6752b3d25391ac23978b1e5e35ca66b");
const cmap = `/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Fixture-Latin def
/CMapType 2 def
1 begincodespacerange
<00> <FF>
endcodespacerange
2 beginbfchar
<41> <0041>
<42> <0042>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end
`;

const resources = [
  [4, "<< /Font << /F1 6 0 R >> >>"],
  [0, "<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /FirstChar 65 /LastChar 66 " +
    "/Widths [639 650] /Encoding /WinAnsiEncoding /FontDescriptor 7 0 R /ToUnicode 9 0 R >>"],
  [0, "<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 " +
    "/FontBBox [-621 -389 2800 1067] /ItalicAngle 0 /Ascent 1069 /Descent -293 " +
    "/CapHeight 714 /StemV 80 /FontFile2 8 0 R >>"],
  [0, stream(`/Length ${font.length} /Length1 ${font.length}`, font)],
  [0, stream(`/Length ${Buffer.byteLength(cmap)}`, cmap)],
];

const page = (contents, extra = "") =>
  `<< /Type /Page /Parent 2 0 R /Resources 5 4 R /Contents ${contents} 0 R${extra} >>`;
const mediaBox = "/MediaBox [0 0 160 100]";
const paint = (letter) => `BT /F1 18 Tf 24 52 Td (${letter}) Tj ET\n`;
const compress = (letter) => deflateSync(paint(letter), { level: 9, strategy: constants.Z_FIXED });
const first = compress("A");
const second = compress("B");

const notes = classicPdf([
  [2, "<< /Type /Catalog /Pages 2 0 R /FixtureValues [true false +.125 /Review#20Flag] >>"],
  [0, `<< /Type /Pages /Count 2 /Kids [3 3 R 4 0 R] ${mediaBox} >>`],
  [3, page(10, " /Annots 15 0 R")],
  [0, page(12)],
  ...resources,
  [0, stream("/Filter /FlateDecode /Length 11 7 R", first)],
  [7, String(first.length)],
  [0, stream(`/Filter /FlateDecode /Length ${second.length}`, second)],
  [5, "<< /Title (Preservation and notes) /Producer (FlowPDF independent fixtures) >>"],
  [0, "<< /UnreachableDecoy (Unused sample) >>"],
  [0, "[]"],
], "/Root 1 2 R /Info 13 5 R /ID [<7A2B29194DE84059865FC3E05D770AB1> <7A2B29194DE84059865FC3E05D770AB1>]");

const refused = classicPdf([
  [2, "<< /Type /Catalog /Pages 2 0 R /OpenAction [3 3 R /Fit] >>"],
  [0, `<< /Type /Pages /Count 1 /Kids [3 3 R] ${mediaBox} >>`],
  [3, page(10)],
  [0, "<< /Unused true >>"],
  ...resources,
  [0, stream(`/Length ${Buffer.byteLength(paint("A"))}`, paint("A"))],
], "/Root 1 2 R");

const args = process.argv.slice(2);
const check = args[0] === "--check";
if (args.length !== Number(check)) {
  throw new Error("Usage: node scripts/generate-pdf-native-fixtures.mjs [--check]");
}

for (const [name, bytes] of [["notes-preservation.pdf", notes], ["refused.pdf", refused]]) {
  const path = new URL(name, output);
  if (check) {
    let existing;
    try {
      existing = readFileSync(path);
    } catch {
      throw new Error("Fixture unavailable: " + name);
    }
    if (!bytes.equals(existing)) throw new Error("Fixture differs: " + name);
  } else {
    writeFileSync(path, bytes);
  }
}
