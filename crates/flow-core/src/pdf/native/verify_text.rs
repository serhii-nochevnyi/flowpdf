use std::collections::{BTreeMap, BTreeSet};

use ttf_parser::{Face, GlyphId, Rect as GlyphBox, Tag};

use super::super::reader::{PdfObjectRef, PdfReaderLimits, PdfValue};
use super::super::scene::{ContentCursor, ContentToken};
use super::verifier::{
    MAX_COORD, NativeRedactionMark, NativeRedactionPageEvidence,
    NativeVerificationFailure::{Budget, Structure, TextMismatch, UnsupportedProfile},
    POINT, Rect, VerifyResult, contains, intersects, spend, valid_rect,
};
use super::verify_structure::{
    Dict, Document, Side, coverage, decimal, get, name, number, rect, reference, reject_comments,
    token,
};

const MAX_MAP: usize = 4096;
const MAX_DEPTH: usize = 8;
const MAX_STREAM: usize = 512 * 1024;
const MAX_TEXT_WORK: usize = 8_000_000;
const MAX_GLYPHS_PER_SHOW: usize = 4_096;
const MAX_FONT_WORK: usize = 1_000_000;

pub(super) fn verify(
    accepted: &[u8],
    candidate: &[u8],
    order: &[u64],
    marks: &[NativeRedactionMark],
) -> VerifyResult<Vec<NativeRedactionPageEvidence>> {
    let accepted = Document::open(accepted, Side::Accepted)?;
    let candidate = Document::open(candidate, Side::Candidate)?;
    let covered = coverage(&accepted, &candidate, order, marks)?;
    let mut before = Extraction::new();
    let mut after = Extraction::new();
    let mut removed_sources = BTreeSet::new();
    let mut removed_streams = BTreeSet::new();
    let mut matching_sources = BTreeSet::new();
    let mut removed_operators = BTreeSet::new();
    let mut retained_operators = BTreeSet::new();
    for (index, &page_id) in order.iter().enumerate() {
        let old_page = accepted.page(index)?;
        let new_page = candidate.page(index)?;
        let mut expected = before.page(&accepted, &old_page)?;
        let found = after.page(&candidate, &new_page)?;
        let page_marks = marks
            .iter()
            .filter(|mark| mark.page_id == page_id)
            .collect::<Vec<_>>();
        spend(
            &mut before.work,
            expected
                .len()
                .checked_mul(page_marks.len() + 1)
                .ok_or(Budget)?,
        )?;
        expected.retain(|run| {
            let removed = page_marks
                .iter()
                .any(|mark| intersects(mark.rect, run.bounds));
            if removed {
                removed_streams.insert(before.stream_digests[&run.source.0]);
                removed_sources.insert(run.source.0);
                removed_operators.insert(run.source);
            } else {
                retained_operators.insert(run.source);
            }
            !removed
        });
        if !expected
            .iter()
            .map(|run| &run.signature)
            .eq(found.iter().map(|run| &run.signature))
        {
            return Err(TextMismatch);
        }
        for (left, right) in expected.iter().zip(&found) {
            let source = left.source.0;
            let result = right.source.0;
            if before.stream_digests[&source] == after.stream_digests[&result] {
                matching_sources.insert((source, result));
            }
        }
    }
    let independently_retained: BTreeSet<_> = matching_sources
        .into_iter()
        .filter_map(|(source, result)| (!removed_sources.contains(&source)).then_some(result))
        .collect();
    if !removed_operators.is_disjoint(&retained_operators)
        || after.stream_digests.iter().any(|(id, hash)| {
            removed_streams.contains(hash) && !independently_retained.contains(id)
        })
    {
        return Err(TextMismatch);
    }
    Ok(covered)
}

#[derive(PartialEq, Eq)]
struct Signature {
    text: String,
    font: [u8; 32],
    matrix: Matrix,
    origin: [i64; 2],
    size: i64,
}

struct Run {
    signature: Signature,
    bounds: Rect,
    source: (PdfObjectRef, usize),
}

struct Extraction<'a> {
    fonts: Fonts<'a>,
    work: usize,
    stream_digests: BTreeMap<PdfObjectRef, [u8; 32]>,
}

impl<'a> Extraction<'a> {
    fn new() -> Self {
        Self {
            fonts: Fonts::new(),
            work: MAX_TEXT_WORK,
            stream_digests: BTreeMap::new(),
        }
    }

    fn page(
        &mut self,
        doc: &'a Document,
        page: &super::verify_structure::Page<'a>,
    ) -> VerifyResult<Vec<Run>> {
        let mut output = Vec::new();
        let mut matrix = Matrix::IDENTITY;
        for &stream in &page.contents {
            matrix = self.stream(
                doc,
                stream,
                page.resources,
                matrix,
                page.geometry.crop_box,
                0,
                &mut output,
            )?;
        }
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    fn stream(
        &mut self,
        doc: &'a Document,
        id: PdfObjectRef,
        resources: Option<&'a Dict>,
        mut matrix: Matrix,
        clip: Rect,
        depth: usize,
        output: &mut Vec<Run>,
    ) -> VerifyResult<Matrix> {
        if depth > MAX_DEPTH {
            return Err(UnsupportedProfile);
        }
        let stream = doc.stream(id)?;
        spend(&mut self.work, stream.data.len() + 1)?;
        self.stream_digests
            .entry(id)
            .or_insert_with(|| *blake3::hash(&stream.data).as_bytes());
        let mut stack = Vec::new();
        let mut text = None;
        let mut operation = 0;
        let mut available = self.work;
        operations(&stream.data, doc.side, &mut available, |op, args, cost| {
            spend(&mut self.work, cost)?;
            operation += 1;
            if let Some(state) = &mut text {
                if op == "ET" && args.is_empty() {
                    text = None;
                } else if matches!(op, "g" | "G" | "rg" | "RG" | "k" | "K") {
                    return Ok(());
                } else if let Some(show) = text_operator(state, op, args)? {
                    let font_id = resource(doc, resources, "Font", show.font)?;
                    let font = self.fonts.get(doc, font_id)?;
                    spend(&mut self.work, show.codes.len() * 4 + 1)?;
                    let layout = font.layout(show.codes, show.origin, show.size)?;
                    let bounds = matrix.bounds(layout.bounds)?;
                    if !contains(clip, bounds) {
                        return Err(UnsupportedProfile);
                    }
                    output.push(Run {
                        signature: Signature {
                            text: layout.glyphs.iter().map(|glyph| glyph.scalar).collect(),
                            font: font.digest,
                            matrix,
                            origin: show.origin,
                            size: show.size,
                        },
                        bounds,
                        source: (id, operation),
                    });
                }
                return Ok(());
            }
            match op {
                "BT" if args.is_empty() => text = Some(TextState::default()),
                "q" if args.is_empty() && stack.len() < 32 => stack.push(matrix),
                "Q" if args.is_empty() => matrix = stack.pop().ok_or(UnsupportedProfile)?,
                "cm" => matrix = matrix.compose(Matrix::new(numbers(args)?)?)?,
                "Do" => {
                    let target = resource(doc, resources, "XObject", operand_name(args)?)?;
                    let dict = &doc.stream(target)?.dict;
                    if name(dict, "Subtype") == Some("Form") {
                        let (matrix, form_clip, resources) =
                            form(doc, dict, resources, matrix, clip)?;
                        self.stream(doc, target, resources, matrix, form_clip, depth + 1, output)?;
                    }
                }
                "g" | "G" | "rg" | "RG" | "k" | "K" | "w" | "m" | "l" | "re" | "h" | "f" | "F"
                | "f*" | "S" | "s" | "n" | "B" | "B*" | "b" | "b*" => {}
                _ => return Err(UnsupportedProfile),
            }
            Ok(())
        })?;
        if text.is_some() || !stack.is_empty() {
            return Err(UnsupportedProfile);
        }
        Ok(matrix)
    }
}

pub(super) struct Operand<'a> {
    pub token: ContentToken,
    pub raw: &'a [u8],
}

pub(super) fn operations(
    data: &[u8],
    side: Side,
    available: &mut usize,
    mut visit: impl FnMut(&str, &[Operand<'_>], usize) -> VerifyResult<()>,
) -> VerifyResult<()> {
    if data.len() > MAX_STREAM {
        return Err(Budget);
    }
    spend(available, data.len() + 1)?;
    if side == Side::Candidate {
        reject_comments(data)?;
    }
    let mut cursor = ContentCursor::new(
        data,
        PdfReaderLimits {
            max_content_tokens: 16_384,
            max_nesting: 2,
            max_array_items: 4096,
            max_string_bytes: 2 * MAX_GLYPHS_PER_SHOW,
            ..Default::default()
        },
    );
    let mut args = Vec::new();
    let mut previous = 0;
    while let Some((token, start)) = cursor.next().map_err(|_| UnsupportedProfile)? {
        let end = cursor.position();
        if let ContentToken::Word(op) = token {
            visit(&op, &args, end - previous + 1)?;
            previous = end;
            args.clear();
        } else {
            if args.len() == 8 {
                return Err(UnsupportedProfile);
            }
            args.push(Operand {
                token,
                raw: &data[start..end],
            });
        }
    }
    if !args.is_empty() {
        return Err(UnsupportedProfile);
    }
    Ok(())
}

pub(super) fn numbers<const N: usize>(args: &[Operand<'_>]) -> VerifyResult<[i64; N]> {
    let mut values = [0; N];
    if args.len() != N {
        return Err(UnsupportedProfile);
    }
    for (value, operand) in values.iter_mut().zip(args) {
        if !matches!(
            operand.token,
            ContentToken::Integer(_) | ContentToken::Number(_)
        ) {
            return Err(UnsupportedProfile);
        }
        *value = decimal(operand.raw)?;
    }
    Ok(values)
}

pub(super) fn operand_name<'a>(args: &'a [Operand<'_>]) -> VerifyResult<&'a str> {
    let [
        Operand {
            token: ContentToken::Name(name),
            ..
        },
    ] = args
    else {
        return Err(UnsupportedProfile);
    };
    safe_name(name)
}

fn safe_name(name: &str) -> VerifyResult<&str> {
    if name.is_empty() || !name.bytes().all(|c| c.is_ascii_graphic() && c != b'#') {
        return Err(UnsupportedProfile);
    }
    Ok(name)
}

pub(super) fn resource(
    doc: &Document,
    resources: Option<&Dict>,
    kind: &str,
    key: &str,
) -> VerifyResult<PdfObjectRef> {
    let table = doc.dictionary(get(resources.ok_or(Structure)?, kind)?)?;
    reference(get(table, key)?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Matrix(pub [i64; 6]);

impl Matrix {
    pub const IDENTITY: Self = Self([POINT, 0, 0, POINT, 0, 0]);

    pub fn new(values: [i64; 6]) -> VerifyResult<Self> {
        let [a, b, c, d, _, _] = values;
        if values
            .into_iter()
            .any(|value| !(-MAX_COORD..=MAX_COORD).contains(&value))
            || !((b == 0 && c == 0 && a * d > 0) || (a == 0 && d == 0 && b * c < 0))
        {
            return Err(UnsupportedProfile);
        }
        Ok(Self(values))
    }

    pub fn compose(self, inner: Self) -> VerifyResult<Self> {
        let [a, b, c, d, _, _] = self.0;
        let [g, h, i, j, x, y] = inner.0;
        let [x, y] = self.point([x, y])?;
        Self::new([
            exact_scale(a * g + c * h)?,
            exact_scale(b * g + d * h)?,
            exact_scale(a * i + c * j)?,
            exact_scale(b * i + d * j)?,
            x,
            y,
        ])
    }

    pub fn numerator(self, [x, y]: [i64; 2]) -> [i64; 2] {
        let [a, b, c, d, u, v] = self.0;
        [a * x + c * y + u * POINT, b * x + d * y + v * POINT]
    }

    pub fn point(self, point: [i64; 2]) -> VerifyResult<[i64; 2]> {
        let [x, y] = self.numerator(point);
        Ok([exact_scale(x)?, exact_scale(y)?])
    }

    pub fn bounds(self, [l, b, r, t]: Rect) -> VerifyResult<Rect> {
        let [x, y] = self.numerator([l, b]);
        let [u, v] = self.numerator([r, t]);
        let rounded = [
            x.min(u).div_euclid(POINT),
            y.min(v).div_euclid(POINT),
            -(-x.max(u)).div_euclid(POINT),
            -(-y.max(v)).div_euclid(POINT),
        ];
        valid_rect(rounded)
            .then_some(rounded)
            .ok_or(UnsupportedProfile)
    }

    fn restrict_clip(self, [l, b, r, t]: Rect, parent: Rect) -> VerifyResult<Rect> {
        let [x, y] = self.numerator([l, b]);
        let [u, v] = self.numerator([r, t]);
        let clip = [
            parent[0].max(-(-x.min(u)).div_euclid(POINT)),
            parent[1].max(-(-y.min(v)).div_euclid(POINT)),
            parent[2].min(x.max(u).div_euclid(POINT)),
            parent[3].min(y.max(v).div_euclid(POINT)),
        ];
        valid_rect(clip).then_some(clip).ok_or(UnsupportedProfile)
    }
}

fn exact_scale(value: i64) -> VerifyResult<i64> {
    if value % POINT != 0 {
        return Err(UnsupportedProfile);
    }
    let scaled = value / POINT;
    (-MAX_COORD..=MAX_COORD)
        .contains(&scaled)
        .then_some(scaled)
        .ok_or(UnsupportedProfile)
}

pub(super) fn form<'a>(
    doc: &'a Document,
    dict: &'a Dict,
    inherited: Option<&'a Dict>,
    outer: Matrix,
    parent_clip: Rect,
) -> VerifyResult<(Matrix, Rect, Option<&'a Dict>)> {
    if dict
        .get("FormType")
        .is_some_and(|v| v.as_integer() != Some(1))
    {
        return Err(UnsupportedProfile);
    }
    let matrix = if let Some(value) = dict.get("Matrix") {
        let [a, b, c, d, e, f] = value.as_array().ok_or(Structure)? else {
            return Err(Structure);
        };
        outer.compose(Matrix::new([
            number(a)?,
            number(b)?,
            number(c)?,
            number(d)?,
            number(e)?,
            number(f)?,
        ])?)?
    } else {
        outer
    };
    let clip = matrix.restrict_clip(rect(get(dict, "BBox")?)?, parent_clip)?;
    let resources = match dict.get("Resources") {
        Some(value) => Some(doc.dictionary(value)?),
        None => inherited,
    };
    Ok((matrix, clip, resources))
}

#[derive(Default)]
pub(super) struct TextState {
    font: Option<(String, i64)>,
    origin: Option<[i64; 2]>,
    shown: bool,
}

pub(super) struct Show<'a> {
    pub font: &'a str,
    pub size: i64,
    pub origin: [i64; 2],
    pub codes: &'a [u8],
}

pub(super) fn text_operator<'a>(
    state: &'a mut TextState,
    op: &str,
    args: &'a [Operand<'_>],
) -> VerifyResult<Option<Show<'a>>> {
    match op {
        "Tf" if !state.shown => {
            let [
                Operand {
                    token: ContentToken::Name(font),
                    ..
                },
                size,
            ] = args
            else {
                return Err(UnsupportedProfile);
            };
            let [size] = numbers(std::slice::from_ref(size))?;
            if !(1..=10_000 * POINT).contains(&size) {
                return Err(UnsupportedProfile);
            }
            state.font = Some((safe_name(font)?.to_owned(), size));
        }
        "Td" | "Tm" if !state.shown && state.origin.is_none() => {
            state.origin = Some(if op == "Td" {
                numbers(args)?
            } else {
                let [a, b, c, d, x, y] = numbers(args)?;
                if [a, b, c, d] != [POINT, 0, 0, POINT] {
                    return Err(UnsupportedProfile);
                }
                [x, y]
            });
        }
        "Tj" | "TJ" if !state.shown => {
            let bytes = match (op, args) {
                (
                    "Tj",
                    [
                        Operand {
                            token: ContentToken::String(bytes),
                            ..
                        },
                    ],
                ) => bytes,
                (
                    "TJ",
                    [
                        Operand {
                            token: ContentToken::Array(items),
                            ..
                        },
                    ],
                ) => {
                    let mut bytes = None;
                    for item in items {
                        match item {
                            ContentToken::String(value) if bytes.is_none() => bytes = Some(value),
                            ContentToken::Integer(0) => {}
                            _ => return Err(UnsupportedProfile),
                        }
                    }
                    bytes.ok_or(UnsupportedProfile)?
                }
                _ => return Err(UnsupportedProfile),
            };
            let (font, size) = state.font.as_ref().ok_or(UnsupportedProfile)?;
            if bytes.is_empty() || bytes.len() > 2 * MAX_GLYPHS_PER_SHOW {
                return Err(UnsupportedProfile);
            }
            state.shown = true;
            return Ok(Some(Show {
                font,
                size: *size,
                origin: state.origin.unwrap_or_default(),
                codes: bytes,
            }));
        }
        "Tr" | "Tc" | "Tw" | "Ts" if numbers(args)? == [0] => {}
        "Tz" if numbers(args)? == [100 * POINT] => {}
        _ => return Err(UnsupportedProfile),
    }
    Ok(None)
}

pub(super) struct Fonts<'a> {
    keyed: BTreeMap<PdfObjectRef, Font<'a>>,
    work: usize,
}

impl<'a> Fonts<'a> {
    pub fn new() -> Self {
        Self {
            keyed: BTreeMap::new(),
            work: MAX_FONT_WORK,
        }
    }

    pub fn get(&mut self, document: &'a Document, id: PdfObjectRef) -> VerifyResult<&Font<'a>> {
        use std::collections::btree_map::Entry;
        match self.keyed.entry(id) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => Ok(entry.insert(Font::read(document, id, &mut self.work)?)),
        }
    }
}

struct Character {
    scalar: char,
    id: GlyphId,
    width: i64,
    ink: Option<GlyphBox>,
}

pub(super) struct Font<'a> {
    pub face: Face<'a>,
    pub digest: [u8; 32],
    characters: BTreeMap<u16, Character>,
    code_bytes: usize,
}

pub(super) struct Glyph {
    pub scalar: char,
    pub id: GlyphId,
    pub offset: i64,
    pub has_ink: bool,
}

pub(super) struct Layout {
    pub bounds: Rect,
    pub glyphs: Vec<Glyph>,
}

impl<'a> Font<'a> {
    fn read(doc: &'a Document, id: PdfObjectRef, work: &mut usize) -> VerifyResult<Self> {
        let root = doc.dict(id)?;
        let (metrics, code_bytes) = match (name(root, "Subtype"), name(root, "Encoding")) {
            (Some("TrueType"), Some("WinAnsiEncoding")) => (root, 1),
            (Some("Type0"), Some("Identity-H")) => {
                let [child] = get(root, "DescendantFonts")?.as_array().ok_or(Structure)? else {
                    return Err(Structure);
                };
                let child = doc.dictionary(child)?;
                if name(child, "Subtype") != Some("CIDFontType2")
                    || name(root, "BaseFont") != name(child, "BaseFont")
                    || !child.contains_key("CIDSystemInfo")
                {
                    return Err(UnsupportedProfile);
                }
                (child, 2)
            }
            _ => return Err(UnsupportedProfile),
        };
        let descriptor = doc.dictionary(get(metrics, "FontDescriptor")?)?;
        let font_name = name(metrics, "BaseFont").ok_or(UnsupportedProfile)?;
        if name(descriptor, "FontName") != Some(font_name)
            || (code_bytes == 1
                && get(descriptor, "Flags")?.as_integer().map(|n| n & 36) != Some(32))
        {
            return Err(UnsupportedProfile);
        }
        let data = &doc.stream(reference(get(descriptor, "FontFile2")?)?)?.data;
        if !data.starts_with(&[0, 1, 0, 0]) {
            return Err(UnsupportedProfile);
        }
        spend(work, data.len() / 16 + 1)?;
        let face = Face::parse(data, 0).map_err(|_| UnsupportedProfile)?;
        let tables = face.tables();
        let glyf = tables.glyf.ok_or(UnsupportedProfile)?;
        let raw = face.raw_face();
        let cmap = tables.cmap.ok_or(UnsupportedProfile)?;
        if cmap.subtables.len() > 32 {
            return Err(UnsupportedProfile);
        }
        let windows = cmap.subtables.into_iter().find(|table| {
            table.platform_id == ttf_parser::PlatformId::Windows && table.encoding_id == 1
        });
        let glyph_map = match metrics.get("CIDToGIDMap") {
            Some(value) if value.as_name() != Some("Identity") => {
                let bytes = &doc.stream(reference(value)?)?.data;
                if bytes.is_empty() || bytes.len() % 2 != 0 || bytes.len() > 131_072 {
                    return Err(UnsupportedProfile);
                }
                spend(work, bytes.len())?;
                let mut unique = BTreeSet::new();
                for pair in bytes.chunks_exact(2) {
                    let glyph = u16::from_be_bytes([pair[0], pair[1]]);
                    if glyph >= face.number_of_glyphs() || (glyph != 0 && !unique.insert(glyph)) {
                        return Err(UnsupportedProfile);
                    }
                }
                Some(bytes.as_slice())
            }
            _ => None,
        };
        let (default_width, widths) = read_widths(doc, metrics, code_bytes)?;
        let unicode = &doc.stream(reference(get(root, "ToUnicode")?)?)?.data;
        spend(work, unicode.len())?;
        let mappings = read_cmap(unicode, code_bytes)?;
        let loca = ttf_parser::loca::Table::parse(
            tables.maxp.number_of_glyphs,
            tables.head.index_to_location_format,
            raw.table(Tag::from_bytes(b"loca"))
                .ok_or(UnsupportedProfile)?,
        )
        .ok_or(UnsupportedProfile)?;
        let glyph_data = raw
            .table(Tag::from_bytes(b"glyf"))
            .ok_or(UnsupportedProfile)?;
        let mut characters = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let em = i64::from(face.units_per_em());
        for (code, scalar) in mappings {
            spend(work, 32)?;
            let glyph = match (code_bytes, glyph_map) {
                (1, _) if win_ansi(code) == Some(scalar) => windows
                    .and_then(|table| table.glyph_index(u32::from(scalar)))
                    .ok_or(UnsupportedProfile)?,
                (2, Some(map)) => {
                    let offset = usize::from(code) * 2;
                    let pair = map.get(offset..offset + 2).ok_or(UnsupportedProfile)?;
                    GlyphId(u16::from_be_bytes([pair[0], pair[1]]))
                }
                (2, None) => GlyphId(code),
                _ => return Err(UnsupportedProfile),
            };
            if glyph.0 == 0
                || glyph.0 >= face.number_of_glyphs()
                || !seen.insert(glyph.0)
                || face.glyph_index(scalar) != Some(glyph)
                || cmap
                    .subtables
                    .into_iter()
                    .filter(|table| table.is_unicode())
                    .filter_map(|table| table.glyph_index(u32::from(scalar)))
                    .any(|other| other != glyph)
            {
                return Err(UnsupportedProfile);
            }
            let width = widths.get(&code).copied().unwrap_or(default_width);
            let advance = i64::from(face.glyph_hor_advance(glyph).ok_or(UnsupportedProfile)?);
            if advance == 0
                || width == 0
                || (width * em - advance * POINT * POINT).abs() > (POINT / 2) * em
            {
                return Err(UnsupportedProfile);
            }
            let ink = if scalar == ' ' && empty_glyph(loca, glyph) {
                None
            } else {
                bound_glyph(loca, glyph_data, glyph, 0, work)?;
                let header = glyf.bbox(glyph).ok_or(UnsupportedProfile)?;
                let measured = face.glyph_bounding_box(glyph).ok_or(UnsupportedProfile)?;
                if header.x_min > measured.x_min
                    || header.y_min > measured.y_min
                    || header.x_max < measured.x_max
                    || header.y_max < measured.y_max
                    || header.x_min >= header.x_max
                    || header.y_min >= header.y_max
                {
                    return Err(UnsupportedProfile);
                }
                Some(header)
            };
            characters.insert(
                code,
                Character {
                    scalar,
                    id: glyph,
                    width,
                    ink,
                },
            );
        }
        let mut hasher = blake3::Hasher::new();
        hash_value(doc, doc.object(id)?, &mut hasher, work, 0)?;
        Ok(Self {
            face,
            digest: *hasher.finalize().as_bytes(),
            characters,
            code_bytes,
        })
    }

    pub fn layout(&self, codes: &[u8], [x, y]: [i64; 2], size: i64) -> VerifyResult<Layout> {
        if !codes.len().is_multiple_of(self.code_bytes)
            || codes.len() / self.code_bytes > MAX_GLYPHS_PER_SHOW
        {
            return Err(UnsupportedProfile);
        }
        let em = i128::from(self.face.units_per_em());
        let units = i128::from(POINT * POINT);
        let divisor = em * units;
        let size = i128::from(size);
        let mut glyphs = Vec::with_capacity(codes.len() / self.code_bytes);
        let mut pen = 0_i128;
        let [x, y] = [i128::from(x) * divisor, i128::from(y) * divisor];
        let mut bounds = [x, i128::MAX, x, i128::MIN];
        for code in codes.chunks_exact(self.code_bytes) {
            let code = if let [high, low] = *code {
                u16::from_be_bytes([high, low])
            } else {
                u16::from(code[0])
            };
            let character = self.characters.get(&code).ok_or(UnsupportedProfile)?;
            let at = x + pen;
            if let Some(ink) = character.ink {
                let scale = |edge| i128::from(edge) * size * units;
                bounds[0] = bounds[0].min(at + scale(ink.x_min));
                bounds[1] = bounds[1].min(y + scale(ink.y_min));
                bounds[2] = bounds[2].max(at + scale(ink.x_max));
                bounds[3] = bounds[3].max(y + scale(ink.y_max));
            }
            glyphs.push(Glyph {
                scalar: character.scalar,
                id: character.id,
                offset: i64::try_from((at + divisor / 2).div_euclid(divisor))
                    .map_err(|_| UnsupportedProfile)?,
                has_ink: character.ink.is_some(),
            });
            pen += i128::from(character.width) * size * em;
        }
        bounds[2] = bounds[2].max(x + pen);
        if bounds[1] == i128::MAX {
            return Err(UnsupportedProfile);
        }
        let b = [
            bounds[0].div_euclid(divisor),
            bounds[1].div_euclid(divisor),
            -(-bounds[2]).div_euclid(divisor),
            -(-bounds[3]).div_euclid(divisor),
        ]
        .map(|edge| i64::try_from(edge).unwrap_or(i64::MAX));
        if !valid_rect(b) {
            return Err(UnsupportedProfile);
        }
        Ok(Layout { bounds: b, glyphs })
    }
}

fn empty_glyph(loca: ttf_parser::loca::Table<'_>, glyph: GlyphId) -> bool {
    let (left, right) = (glyph.0, glyph.0 + 1);
    match loca {
        ttf_parser::loca::Table::Short(offsets) => offsets
            .get(left)
            .zip(offsets.get(right))
            .is_some_and(|(l, r)| l == r),
        ttf_parser::loca::Table::Long(offsets) => offsets
            .get(left)
            .zip(offsets.get(right))
            .is_some_and(|(l, r)| l == r),
    }
}

fn bound_glyph(
    loca: ttf_parser::loca::Table<'_>,
    data: &[u8],
    glyph: GlyphId,
    depth: usize,
    work: &mut usize,
) -> VerifyResult<()> {
    if depth > MAX_DEPTH {
        return Err(UnsupportedProfile);
    }
    let range = loca.glyph_range(glyph).ok_or(UnsupportedProfile)?;
    let glyph_data = data.get(range).ok_or(UnsupportedProfile)?;
    let word = |offset: usize| {
        glyph_data
            .get(offset..offset + 2)
            .and_then(|word| word.try_into().ok())
            .map(u16::from_be_bytes)
            .ok_or(UnsupportedProfile)
    };
    spend(work, 1)?;
    match word(0)? as i16 {
        count @ 1.. => spend(work, 1 + usize::from(word(8 + 2 * count as usize)?)),
        -1 => {
            let mut pos = 10;
            loop {
                let flags = word(pos)?;
                if flags & 2 == 0 || flags & !0x1727 != 0 {
                    return Err(UnsupportedProfile);
                }
                bound_glyph(loca, data, GlyphId(word(pos + 2)?), depth + 1, work)?;
                pos += if flags & 1 == 0 { 6 } else { 8 };
                if flags & 0x20 == 0 {
                    return (pos <= glyph_data.len())
                        .then_some(())
                        .ok_or(UnsupportedProfile);
                }
            }
        }
        _ => Err(UnsupportedProfile),
    }
}

fn read_widths(
    doc: &Document,
    dict: &Dict,
    code_bytes: usize,
) -> VerifyResult<(i64, BTreeMap<u16, i64>)> {
    let width = |v| {
        number(v).and_then(|width| {
            (1..=10_000 * POINT)
                .contains(&width)
                .then_some(width)
                .ok_or(UnsupportedProfile)
        })
    };
    let mut widths = BTreeMap::new();
    if code_bytes == 1 {
        let first = get(dict, "FirstChar")?
            .as_integer()
            .filter(|n| (0..=255).contains(n))
            .ok_or(UnsupportedProfile)?;
        let last = get(dict, "LastChar")?
            .as_integer()
            .filter(|n| (first..=255).contains(n))
            .ok_or(UnsupportedProfile)?;
        let all = doc
            .resolve(get(dict, "Widths")?)?
            .as_array()
            .ok_or(Structure)?;
        if all.len() != (last - first + 1) as usize {
            return Err(UnsupportedProfile);
        }
        for (code, v) in (first..=last).zip(all) {
            let value = number(v)?;
            if !(0..=10_000 * POINT).contains(&value) {
                return Err(UnsupportedProfile);
            }
            widths.insert(code as u16, value);
        }
        return Ok((0, widths));
    }
    let default = dict
        .get("DW")
        .map(width)
        .transpose()?
        .unwrap_or(1_000 * POINT);
    let Some(items) = dict.get("W") else {
        return Ok((default, widths));
    };
    let mut items = doc.resolve(items)?.as_array().ok_or(Structure)?;
    while let [start, next, tail @ ..] = items {
        let start =
            u16::try_from(start.as_integer().ok_or(Structure)?).map_err(|_| UnsupportedProfile)?;
        let mut add = |code, value| {
            if widths.len() >= MAX_MAP || widths.insert(code, width(value)?).is_some() {
                return Err(UnsupportedProfile);
            }
            Ok(())
        };
        if let Some(list) = next.as_array() {
            if list.is_empty() || list.len() > MAX_MAP {
                return Err(UnsupportedProfile);
            }
            for (index, value) in list.iter().enumerate() {
                add(
                    start.checked_add(index as u16).ok_or(UnsupportedProfile)?,
                    value,
                )?;
            }
            items = tail;
        } else {
            let end = u16::try_from(next.as_integer().ok_or(Structure)?)
                .map_err(|_| UnsupportedProfile)?;
            let [value, tail @ ..] = tail else {
                return Err(Structure);
            };
            if end < start {
                return Err(UnsupportedProfile);
            }
            for code in start..=end {
                add(code, value)?;
            }
            items = tail;
        }
    }
    if !items.is_empty() {
        return Err(Structure);
    }
    Ok((default, widths))
}

fn expect(rest: &mut &[u8], words: &[&[u8]]) -> VerifyResult<()> {
    for word in words {
        if token(rest)? != *word {
            return Err(UnsupportedProfile);
        }
    }
    Ok(())
}

fn cmap_integer(rest: &mut &[u8]) -> VerifyResult<usize> {
    let raw = token(rest)?;
    if !raw.iter().all(u8::is_ascii_digit) {
        return Err(UnsupportedProfile);
    }
    std::str::from_utf8(raw)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .ok_or(UnsupportedProfile)
}

fn hex(rest: &mut &[u8], width: usize) -> VerifyResult<u32> {
    let bytes = token(rest)?;
    if bytes.len() != width * 2 + 2
        || bytes[0] != b'<'
        || bytes[bytes.len() - 1] != b'>'
        || !bytes[1..bytes.len() - 1].iter().all(u8::is_ascii_hexdigit)
    {
        return Err(UnsupportedProfile);
    }
    std::str::from_utf8(&bytes[1..bytes.len() - 1])
        .ok()
        .and_then(|raw| u32::from_str_radix(raw, 16).ok())
        .ok_or(UnsupportedProfile)
}

fn unicode(rest: &mut &[u8], offset: u32) -> VerifyResult<char> {
    let raw = token(rest)?;
    let mut again = raw;
    let scalar = match raw.len() {
        6 => {
            let code = hex(&mut again, 2)?
                .checked_add(offset)
                .ok_or(UnsupportedProfile)?;
            if code > 0xffff {
                return Err(UnsupportedProfile);
            }
            code
        }
        10 => {
            let pair = hex(&mut again, 4)?
                .checked_add(offset)
                .ok_or(UnsupportedProfile)?;
            let [a, b, c, d] = pair.to_be_bytes();
            let lead = u32::from(u16::from_be_bytes([a, b]));
            let tail = u32::from(u16::from_be_bytes([c, d]));
            if !(0xd800..=0xdbff).contains(&lead) || !(0xdc00..=0xdfff).contains(&tail) {
                return Err(UnsupportedProfile);
            }
            0x10000 + ((lead - 0xd800) << 10) + tail - 0xdc00
        }
        _ => return Err(UnsupportedProfile),
    };
    char::from_u32(scalar)
        .filter(|ch| !ch.is_control())
        .ok_or(UnsupportedProfile)
}

fn read_cmap(mut rest: &[u8], width: usize) -> VerifyResult<BTreeMap<u16, char>> {
    if rest.len() > 128 * 1024 || !rest.is_ascii() {
        return Err(UnsupportedProfile);
    }
    expect(
        &mut rest,
        &[b"/CIDInit", b"/ProcSet", b"findresource", b"begin"],
    )?;
    if cmap_integer(&mut rest)? > 100 {
        return Err(UnsupportedProfile);
    }
    expect(&mut rest, &[b"dict", b"begin", b"begincmap"])?;
    let mut metadata = BTreeSet::new();
    let mut codes = BTreeMap::new();
    let mut scalars = BTreeSet::new();
    let mut code_space = false;
    let mut declared_mappings = None;
    loop {
        let mut look = rest;
        let next = token(&mut look)?;
        if next.starts_with(b"/") && !metadata.insert(next) {
            return Err(UnsupportedProfile);
        }
        match next {
            b"endcmap" => {
                rest = look;
                break;
            }
            b"/CMapType" => {
                rest = look;
                expect(&mut rest, &[b"2", b"def"])?;
            }
            b"/CMapName" => {
                rest = look;
                let name = token(&mut rest)?;
                match name {
                    b"/Adobe-Identity-UCS"
                    | b"/Adobe-Identity-UCS2"
                    | b"/Example"
                    | b"/FixtureMap"
                    | b"/Fixture-Latin" => {}
                    _ => {
                        let suffix = name.strip_prefix(b"/FlowPDF-").ok_or(UnsupportedProfile)?;
                        if suffix.is_empty()
                            || suffix.len() > 4
                            || suffix[0] == b'0'
                            || !suffix.iter().all(u8::is_ascii_digit)
                        {
                            return Err(UnsupportedProfile);
                        }
                        declared_mappings = Some(
                            suffix
                                .iter()
                                .fold(0, |count, byte| count * 10 + usize::from(byte - b'0')),
                        );
                    }
                }
                expect(&mut rest, &[b"def"])?;
            }
            b"/CIDSystemInfo" => {
                rest = look;
                expect(&mut rest, &[b"<<", b"/Registry", b"(Adobe)", b"/Ordering"])?;
                if !matches!(token(&mut rest)?, b"(Identity)" | b"(UCS)") {
                    return Err(UnsupportedProfile);
                }
                expect(&mut rest, &[b"/Supplement"])?;
                let supplement = token(&mut rest)?;
                if !matches!(supplement, [b'0'..=b'9'] | b"10") {
                    return Err(UnsupportedProfile);
                }
                expect(&mut rest, &[b">>", b"def"])?;
            }
            _ => {
                let count = cmap_integer(&mut rest)?;
                if count == 0 || count > MAX_MAP {
                    return Err(UnsupportedProfile);
                }
                match token(&mut rest)? {
                    b"begincodespacerange" if count == 1 && !code_space => {
                        if hex(&mut rest, width)? != 0
                            || hex(&mut rest, width)? != (1 << (width * 8)) - 1
                        {
                            return Err(UnsupportedProfile);
                        }
                        expect(&mut rest, &[b"endcodespacerange"])?;
                        code_space = true;
                    }
                    kind @ (b"beginbfchar" | b"beginbfrange") if code_space => {
                        let range = kind == b"beginbfrange";
                        for _ in 0..count {
                            let low = hex(&mut rest, width)?;
                            let high = if range { hex(&mut rest, width)? } else { low };
                            if high < low || high - low + 1 > (MAX_MAP - codes.len()) as u32 {
                                return Err(UnsupportedProfile);
                            }
                            let start = rest;
                            for code in low..=high {
                                rest = start;
                                let scalar = unicode(&mut rest, code - low)?;
                                if codes.insert(code as u16, scalar).is_some()
                                    || !scalars.insert(scalar)
                                {
                                    return Err(UnsupportedProfile);
                                }
                            }
                        }
                        expect(
                            &mut rest,
                            &[if range { b"endbfrange" } else { b"endbfchar" }],
                        )?;
                    }
                    _ => return Err(UnsupportedProfile),
                }
            }
        }
    }
    expect(
        &mut rest,
        &[
            b"CMapName",
            b"currentdict",
            b"/CMap",
            b"defineresource",
            b"pop",
            b"end",
            b"end",
        ],
    )?;
    if !rest
        .iter()
        .all(|&byte| super::verify_structure::pdf_space(byte))
        || codes.is_empty()
        || declared_mappings.is_some_and(|count| count != codes.len())
        || !metadata.contains(b"/CMapName".as_slice())
        || !metadata.contains(b"/CMapType".as_slice())
    {
        return Err(UnsupportedProfile);
    }
    Ok(codes)
}

fn win_ansi(code: u16) -> Option<char> {
    const EXTENDED: [char; 32] = [
        '€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0',
        '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\0', 'ž', 'Ÿ',
    ];
    match code {
        32..=126 | 161..=172 | 174..=255 => char::from_u32(u32::from(code)),
        128..=159 => Some(EXTENDED[usize::from(code - 128)]).filter(|ch| *ch != '\0'),
        _ => None,
    }
}

fn hash_value(
    doc: &Document,
    value: &PdfValue,
    hasher: &mut blake3::Hasher,
    work: &mut usize,
    depth: usize,
) -> VerifyResult<()> {
    if depth > 12 {
        return Err(UnsupportedProfile);
    }
    spend(work, 1)?;
    let mut hash_bytes = |tag, bytes: &[u8]| {
        hasher.update(&[tag]);
        hasher.update(&(bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    };
    match doc.resolve(value)? {
        PdfValue::Name(name) => hash_bytes(1, name.as_bytes()),
        PdfValue::String(bytes) => hash_bytes(2, bytes),
        PdfValue::Integer(n) => hash_bytes(3, &n.to_be_bytes()),
        PdfValue::ExactReal(raw) => hash_bytes(4, raw.as_bytes()),
        PdfValue::Array(items) => {
            hasher.update(&[5]);
            hasher.update(&(items.len() as u64).to_be_bytes());
            for item in items {
                hash_value(doc, item, hasher, work, depth + 1)?;
            }
        }
        PdfValue::Dictionary(dict) => {
            hasher.update(&[6]);
            hasher.update(&(dict.len() as u64).to_be_bytes());
            for (key, value) in dict {
                hasher.update(&(key.len() as u64).to_be_bytes());
                hasher.update(key.as_bytes());
                hash_value(doc, value, hasher, work, depth + 1)?;
            }
        }
        PdfValue::Stream(stream) => {
            spend(work, stream.data.len() / 16 + 1)?;
            hash_bytes(7, &stream.data);
        }
        _ => return Err(UnsupportedProfile),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_form_clip_is_restricted_to_the_inner_grid() {
        let matrix = Matrix::new([0, 501, -501, 0, 0, 0]).unwrap();
        assert_eq!(
            matrix.restrict_clip([1, 1, 5, 5], [-2, 0, 2, 2]),
            Ok([-2, 1, -1, 2]),
        );
    }
}
