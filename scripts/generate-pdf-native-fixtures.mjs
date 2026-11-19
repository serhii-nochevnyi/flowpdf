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

const literalStream = (bytes, dictionary = "") =>
  stream(`/Length ${Buffer.byteLength(bytes)} ${dictionary}`.trimEnd(), bytes);
const fixedFlate = (bytes, dictionary) => literalStream(
  deflateSync(bytes, { level: 9, strategy: constants.Z_FIXED }),
  `/Filter /FlateDecode ${dictionary}`,
);

function compositeFont(start) {
  // CIDs 1–3 are A, B and Ї. Glyph IDs and widths come from the pinned font.
  const unicode = cmap.replace("Fixture-Latin", "Fixture-Composite")
    .replace("<00> <FF>", "<0000> <FFFF>")
    .replace("2 beginbfchar\n<41> <0041>\n<42> <0042>",
      "3 beginbfchar\n<0001> <0041>\n<0002> <0042>\n<0003> <0407>");
  return [
    `<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [${start + 1} 0 R] /ToUnicode ${start + 4} 0 R >>`,
    "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular " +
      "/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> " +
      `/W [1 [639 650 339]] /FontDescriptor ${start + 2} 0 R /CIDToGIDMap ${start + 5} 0 R >>`,
    "<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 " +
      "/FontBBox [-621 -389 2800 1067] /ItalicAngle 0 /Ascent 1069 /Descent -293 " +
      `/CapHeight 714 /StemV 80 /FontFile2 ${start + 3} 0 R >>`,
    resources[3][1],
    literalStream(unicode),
    literalStream(Buffer.from([0, 0, 0, 36, 0, 37, 1, 160])),
  ];
}

const grayImage = (samples) => fixedFlate(Buffer.from(samples),
  `/Type /XObject /Subtype /Image /ColorSpace /DeviceGray /Width ${samples.length} /Height 1 /BitsPerComponent 8`);
const flatPdf = (objects, trailer = "") =>
  classicPdf(objects.map((body) => [0, body]), `/Root 1 0 R${trailer}`);
const appearances = "/Type /XObject /Subtype /Form";
const editFamilies = flatPdf([
  "<< /Type /Catalog /Pages 2 0 R /AcroForm 16 0 R /Outlines 24 0 R >>",
  `<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 ${mediaBox} /Resources 7 0 R >>`,
  "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>",
  "<< /Type /Page /Parent 3 0 R /Contents 8 0 R /Annots [17 0 R 18 0 R 19 0 R] >>",
  "<< /Type /Pages /Parent 2 0 R /Kids [6 0 R] /Count 1 /Rotate 90 /CropBox [4 4 156 96] >>",
  "<< /Type /Page /Parent 5 0 R /Contents 8 0 R /Annots [23 0 R] >>",
  "<< /Font << /F1 9 0 R >> /XObject << /Im 15 0 R >> >>",
  literalStream("BT /F1 12 Tf 16 76 Td <000300010001> Tj ET\n" +
    "BT /F1 12 Tf 100 76 Td <0002> Tj ET\n" +
    "1 0 0 rg 16 44 12 12 re f\n" +
    "q 16 0 0 12 48 44 cm /Im Do Q\nq 16 0 0 12 80 44 cm /Im Do Q\n"),
  ...compositeFont(9),
  grayImage([160]),
  "<< /Fields [18 0 R 19 0 R] /DR << /Font << /FlowField 9 0 R >> >> /DA (/FlowField 10 Tf 0 g) >>",
  "<< /Type /Annot /Subtype /Text /P 4 0 R /Rect [128 68 144 84] /Contents (Control note) >>",
  "<< /Type /Annot /Subtype /Widget /P 4 0 R /FT /Tx /T (recipient) /V () /DV () /Rect [16 12 64 32] /AP << /N 20 0 R >> >>",
  "<< /Type /Annot /Subtype /Widget /P 4 0 R /FT /Btn /T (accepted) /V /Off /DV /Off /AS /Off /Rect [80 12 100 32] /AP << /N << /Off 21 0 R /Yes 22 0 R >> >> >>",
  literalStream("q 1 g 0 0 48 20 re f 0 G 1 w 1 1 46 18 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <> Tj ET\nQ\n",
    `${appearances} /BBox [0 0 48 20] /Resources << /Font << /FlowField 9 0 R >> >>`),
  literalStream("0 g 0 0 20 20 re f 1 g 2 2 16 16 re f",
    `${appearances} /BBox [0 0 20 20] /Resources <<>>`),
  literalStream("0 g 0 0 20 20 re f 1 g 2 2 16 16 re f 0 g 6 6 8 8 re f",
    `${appearances} /BBox [0 0 20 20] /Resources <<>>`),
  "<< /Type /Annot /Subtype /Link /P 6 0 R /Rect [120 12 144 32] /Border [0 0 0] /Dest [4 0 R /Fit] >>",
  "<< /Type /Outlines /First 25 0 R /Last 25 0 R /Count 1 >>",
  "<< /Title (First page) /Parent 24 0 R /Dest [4 0 R /Fit] >>",
]);

const metadata = (value) => literalStream(
  `<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:description>${value}</dc:description></rdf:Description></rdf:RDF></x:xmpmeta>`,
  "/Type /Metadata /Subtype /XML",
);
const redaction = flatPdf([
  "<< /Type /Catalog /Pages 2 0 R /Metadata 17 0 R /FlowPDFSource 20 0 R >>",
  `<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 ${mediaBox} >>`,
  "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources 7 0 R /Annots [14 0 R 15 0 R] /Metadata 18 0 R >>",
  "<< /Type /Page /Parent 2 0 R /Contents 6 0 R /Resources << /Font << /F1 8 0 R >> >> >>",
  literalStream("BT /F1 12 Tf 16 76 Td <000300010001> Tj ET\n" +
    "1 0 0 rg 16 44 12 12 re f\n" +
    "q 32 0 0 12 48 44 cm /Im Do Q\n" +
    "0 0 1 rg 112 20 16 12 re f\n"),
  literalStream("BT /F1 12 Tf 16 76 Td <0002> Tj ET\n0 1 0 rg 16 20 16 12 re f\n"),
  "<< /Font << /F1 8 0 R >> /XObject << /Im 16 0 R >> >>",
  ...compositeFont(8),
  "<< /Type /Annot /Subtype /Text /P 3 0 R /Rect [96 68 112 84] /Contents (Sensitive note) >>",
  "<< /Type /Annot /Subtype /Text /P 3 0 R /Rect [128 68 144 84] /Contents (Control note) >>",
  grayImage([217, 83]),
  metadata("Sensitive catalog metadata"),
  metadata("Sensitive page metadata"),
  "<< /Title (Sensitive title) /Author (Sensitive author) >>",
  literalStream("Sensitive source envelope", "/Type /Metadata /Subtype /FlowPDF.Source"),
], " /Info 19 0 R /ID [<7A2B29194DE84059865FC3E05D770AB2> <7A2B29194DE84059865FC3E05D770AB2>]");

const args = process.argv.slice(2);
const check = args[0] === "--check";
if (args.length !== Number(check)) {
  throw new Error("Usage: node scripts/generate-pdf-native-fixtures.mjs [--check]");
}

for (const [name, bytes] of [
  ["notes-preservation.pdf", notes], ["refused.pdf", refused],
  ["edit-families.pdf", editFamilies], ["redaction.pdf", redaction],
]) {
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
