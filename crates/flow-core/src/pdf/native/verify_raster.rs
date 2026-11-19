// @contract: 144 dpi; four fixed subpixel samples per pixel, half-open coverage, nearest image sampling.
use std::collections::{BTreeMap, BTreeSet};

use ttf_parser::OutlineBuilder;

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::ContentToken;
use super::verifier::{
    MAX_COORD, NativeRedactionGeometry, NativeRedactionMark, NativeRedactionPageEvidence,
    NativeVerificationFailure::{Budget, RasterMismatch, Structure, UnsupportedProfile},
    POINT, Rect, VerifyResult, contains, intersects, spend, valid_rect,
};
use super::verify_structure::{Dict, Document, Page, Side, coverage, get, name};
use super::verify_text::{
    Fonts, Matrix, Operand, TextState, form, numbers, operand_name, operations, resource,
    text_operator,
};

const MAX_PAGE_PIXELS: usize = 16_000_000;
const MAX_TOTAL_PIXELS: usize = 48_000_000;
const MAX_IMAGE_DIMENSION: usize = crate::asset::MAX_IMAGE_DIMENSION as usize;
const MAX_RASTER_WORK: usize = 600_000_000;
const MAX_PARSE_WORK: usize = 16_000_000;
const MAX_DRAW_BYTES: usize = 16 * 1024 * 1024;
const MAX_PATH_POINTS: usize = 64;
const MAX_FORM_DEPTH: usize = 8;
const PITCH: i64 = POINT / 2;
const SAMPLE_OFFSETS: [[i64; 2]; 4] = [[125, 125], [375, 125], [125, 375], [375, 375]];

type Point = [i64; 2];
type Pixel = [u8; 3];
const COLOR_UNIT: i64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Color {
    space: u8,
    channels: [i64; 4],
}

impl Color {
    const BLACK: Self = Self {
        space: 1,
        channels: [0; 4],
    };

    fn read(args: &[Operand<'_>], space: u8) -> VerifyResult<Self> {
        if args.len() != usize::from(space) {
            return Err(UnsupportedProfile);
        }
        let mut channels = [0; 4];
        for (value, arg) in channels.iter_mut().zip(args) {
            if !matches!(
                arg.token,
                ContentToken::Integer(_) | ContentToken::Number(_)
            ) {
                return Err(UnsupportedProfile);
            }
            *value = color_component(arg.raw)?;
        }
        Ok(Self { space, channels })
    }

    fn rgb(self) -> Pixel {
        let [a, b, c, k] = self.channels;
        let units = match self.space {
            1 => [a; 3],
            3 => [a, b, c],
            _ => [a, b, c].map(|v| COLOR_UNIT - (v + k).min(COLOR_UNIT)),
        };
        units.map(|v| ((255 * v + COLOR_UNIT / 2) / COLOR_UNIT) as u8)
    }
}

fn color_component(raw: &[u8]) -> VerifyResult<i64> {
    let raw = raw.strip_prefix(b"+").unwrap_or(raw);
    let mut pieces = raw.split(|&b| b == b'.');
    let whole = pieces.next().ok_or(UnsupportedProfile)?;
    let fraction = pieces.next().unwrap_or_default();
    let mut value = 0;
    let mut place = COLOR_UNIT;
    for &b in whole {
        if !b.is_ascii_digit() {
            return Err(UnsupportedProfile);
        }
        value = value * 10 + i64::from(b - b'0') * COLOR_UNIT;
        if value > COLOR_UNIT {
            return Err(UnsupportedProfile);
        }
    }
    for &b in fraction {
        if !b.is_ascii_digit() {
            return Err(UnsupportedProfile);
        }
        place /= 10;
        if place == 0 && b != b'0' {
            return Err(UnsupportedProfile);
        }
        value += i64::from(b - b'0') * place;
    }
    if pieces.next().is_some() || value > COLOR_UNIT {
        return Err(UnsupportedProfile);
    }
    Ok(value)
}

#[derive(Clone, Copy)]
struct Graphics {
    matrix: Matrix,
    fill: Color,
    stroke: Color,
    width: i64,
}

impl Default for Graphics {
    fn default() -> Self {
        Self {
            matrix: Matrix::IDENTITY,
            fill: Color::BLACK,
            stroke: Color::BLACK,
            width: POINT,
        }
    }
}

pub(super) fn verify(
    accepted: &[u8],
    candidate: &[u8],
    order: &[u64],
    marks: &[NativeRedactionMark],
) -> VerifyResult<Vec<NativeRedactionPageEvidence>> {
    let accepted = Document::open(accepted, Side::Accepted)?;
    let candidate = Document::open(candidate, Side::Candidate)?;
    let covered = coverage(&accepted, &candidate, order, marks)?;
    let mut expected = Interpreter::new();
    let mut actual = Interpreter::new();
    let mut pixels_left = MAX_TOTAL_PIXELS;
    let mut work = MAX_RASTER_WORK;
    let mut removed_notes = BTreeSet::new();
    let mut retained_notes = BTreeSet::new();
    let mut removed_sources = BTreeSet::new();
    let mut removed_streams = BTreeSet::new();
    let mut matching_sources = BTreeSet::new();
    let mut removed_operators = BTreeSet::new();
    let mut retained_operators = BTreeSet::new();
    let mut sensitive_images = BTreeSet::new();
    let mut image_replacements = BTreeMap::new();
    for (index, &page_id) in order.iter().enumerate() {
        let original = accepted.page(index)?;
        let result = candidate.page(index)?;
        let marks = marks
            .iter()
            .filter(|mark| mark.page_id == page_id)
            .map(|mark| mark.rect)
            .collect::<Vec<_>>();
        let grid = Grid::new(original.geometry, &mut pixels_left)?;
        let mut source_paint = expected.page(&accepted, &original)?;
        let candidate_paint = actual.page(&candidate, &result)?;
        let mut retained_count = 0;
        for index in 0..source_paint.len() {
            let paint = &mut source_paint[index];
            spend(&mut work, marks.len() + 1)?;
            if let PaintKind::Image(image) = &mut paint.kind {
                image.add_masks(&marks, &mut expected.draw_bytes, &mut work)?;
            } else {
                let mut removed = false;
                for &mark in &marks {
                    if paint.intersects_mark(mark, &mut work)? {
                        removed = true;
                        break;
                    }
                }
                if let PaintKind::Note(note) = &paint.kind {
                    if removed {
                        removed_notes.insert(note.id);
                    } else {
                        retained_notes.insert(note.id);
                    }
                }
                if let Some(operator) = paint.source {
                    if removed {
                        removed_streams.insert(expected.stream_hashes[&operator.0]);
                        removed_sources.insert(operator.0);
                        removed_operators.insert(operator);
                    } else {
                        retained_operators.insert(operator);
                    }
                }
                if removed {
                    continue;
                }
            }
            source_paint.swap(retained_count, index);
            retained_count += 1;
        }
        source_paint.truncate(retained_count);
        if source_paint.len() != candidate_paint.len() {
            return Err(RasterMismatch);
        }
        for (left, right) in source_paint.iter().zip(&candidate_paint) {
            if !left.same_paint(right, &mut work)? {
                return Err(RasterMismatch);
            }
            if let (Some((source, _)), Some((result, _))) = (left.source, right.source) {
                if expected.stream_hashes[&source] == actual.stream_hashes[&result] {
                    matching_sources.insert((source, result));
                }
            }
            if let (PaintKind::Image(before), PaintKind::Image(after)) = (&left.kind, &right.kind) {
                let source = before.data.digest;
                let scrubbed = after.data.digest;
                if *image_replacements.entry(source).or_insert(scrubbed) != scrubbed {
                    return Err(RasterMismatch);
                }
                if source != scrubbed {
                    sensitive_images.insert(source);
                }
            }
        }
        for sample in SAMPLE_OFFSETS {
            let expected_pixels = grid.draw(&source_paint, sample, &mut work)?;
            let candidate_pixels = grid.draw(&candidate_paint, sample, &mut work)?;
            if expected_pixels != candidate_pixels {
                return Err(RasterMismatch);
            }
        }
    }
    let independently_retained: BTreeSet<_> = matching_sources
        .into_iter()
        .filter_map(|(source, result)| (!removed_sources.contains(&source)).then_some(result))
        .collect();
    if !removed_notes.is_disjoint(&retained_notes)
        || !removed_operators.is_disjoint(&retained_operators)
        || actual.stream_hashes.iter().any(|(id, hash)| {
            removed_streams.contains(hash) && !independently_retained.contains(id)
        })
        || !actual.image_hashes.is_disjoint(&sensitive_images)
    {
        return Err(RasterMismatch);
    }
    if candidate.assets().any(|id| !actual.assets.contains(&id)) {
        return Err(Structure);
    }
    Ok(covered)
}

struct Interpreter<'a> {
    fonts: Fonts<'a>,
    parse_work: usize,
    draw_bytes: usize,
    assets: BTreeSet<PdfObjectRef>,
    stream_hashes: BTreeMap<PdfObjectRef, [u8; 32]>,
    image_hashes: BTreeSet<[u8; 32]>,
    image_cache: BTreeMap<PdfObjectRef, ImageData<'a>>,
}

impl<'a> Interpreter<'a> {
    fn new() -> Self {
        Self {
            fonts: Fonts::new(),
            parse_work: MAX_PARSE_WORK,
            draw_bytes: MAX_DRAW_BYTES,
            assets: BTreeSet::new(),
            stream_hashes: BTreeMap::new(),
            image_hashes: BTreeSet::new(),
            image_cache: BTreeMap::new(),
        }
    }

    fn page(&mut self, document: &'a Document, page: &Page<'a>) -> VerifyResult<Vec<Paint<'a>>> {
        self.draw_bytes = MAX_DRAW_BYTES;
        let mut paints = Vec::new();
        let mut graphics = Graphics::default();
        let crop = page.geometry.crop_box;
        for &id in &page.contents {
            graphics = self.stream(document, id, page.resources, graphics, crop, 0, &mut paints)?;
        }
        for &(id, bounds, contents) in &page.annotations {
            spend(&mut self.parse_work, contents.len() + 1)?;
            self.add(
                &mut paints,
                crop,
                Paint {
                    bounds,
                    source: None,
                    kind: PaintKind::Note(Note { id, contents }),
                },
            )?;
        }
        Ok(paints)
    }

    fn add(&mut self, out: &mut Vec<Paint<'a>>, clip: Rect, paint: Paint<'a>) -> VerifyResult<()> {
        if !contains(clip, paint.bounds) {
            return Err(UnsupportedProfile);
        }
        spend(&mut self.draw_bytes, std::mem::size_of::<Paint<'_>>())?;
        out.push(paint);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn stream(
        &mut self,
        document: &'a Document,
        id: PdfObjectRef,
        resources: Option<&'a Dict>,
        mut graphics: Graphics,
        clip: Rect,
        depth: usize,
        paints: &mut Vec<Paint<'a>>,
    ) -> VerifyResult<Graphics> {
        if depth > MAX_FORM_DEPTH {
            return Err(UnsupportedProfile);
        }
        let data = &document.stream(id)?.data;
        spend(&mut self.parse_work, data.len() + 1)?;
        self.stream_hashes
            .entry(id)
            .or_insert_with(|| *blake3::hash(data).as_bytes());
        let mut saved = Vec::new();
        let mut path = Path::default();
        let mut text = None;
        let mut operation = 0;
        let mut available = self.parse_work;
        operations(data, document.side, &mut available, |op, args, cost| {
            spend(&mut self.parse_work, cost)?;
            operation += 1;
            if path.points.is_empty() {
                let color = match op {
                    "g" | "G" => Some(Color::read(args, 1)?),
                    "rg" | "RG" => Some(Color::read(args, 3)?),
                    "k" | "K" => Some(Color::read(args, 4)?),
                    _ => None,
                };
                if let Some(color) = color {
                    if op.as_bytes()[0].is_ascii_lowercase() {
                        graphics.fill = color;
                    } else {
                        graphics.stroke = color;
                    }
                    return Ok(());
                }
            }
            if let Some(state) = &mut text {
                if op == "ET" && args.is_empty() {
                    text = None;
                } else if let Some(show) = text_operator(state, op, args)? {
                    let font_id = resource(document, resources, "Font", show.font)?;
                    self.assets.insert(font_id);
                    let font = self.fonts.get(document, font_id)?;
                    spend(&mut self.parse_work, show.codes.len() * 8 + 1)?;
                    let layout = font.layout(show.codes, show.origin, show.size)?;
                    let bounds = graphics.matrix.bounds(layout.bounds)?;
                    let mut glyph_ids = Vec::new();
                    let mut glyphs = Vec::new();
                    for glyph in layout.glyphs {
                        spend(
                            &mut self.draw_bytes,
                            std::mem::size_of::<ttf_parser::GlyphId>(),
                        )?;
                        glyph_ids.push(glyph.id);
                        if !glyph.has_ink {
                            continue;
                        }
                        let mut outline = Outline::new(
                            [glyph.offset, show.origin[1]],
                            show.size,
                            font.face.units_per_em(),
                            graphics.matrix,
                            &mut self.parse_work,
                            &mut self.draw_bytes,
                        );
                        font.face
                            .outline_glyph(glyph.id, &mut outline)
                            .ok_or(UnsupportedProfile)?;
                        glyphs.push(outline.finish()?);
                    }
                    let paint = TextPaint {
                        font: font.digest,
                        glyph_ids,
                        glyphs,
                        matrix: graphics.matrix,
                        size: show.size,
                        origin: show.origin,
                        color: graphics.fill,
                    };
                    self.add(
                        paints,
                        clip,
                        Paint {
                            bounds,
                            source: Some((id, operation)),
                            kind: PaintKind::Text(paint),
                        },
                    )?;
                }
                return Ok(());
            }
            match op {
                "m" if path.points.is_empty() => path.points.push(numbers(args)?),
                "l" if !path.points.is_empty()
                    && !path.closed
                    && path.points.len() < MAX_PATH_POINTS =>
                {
                    path.points.push(numbers(args)?);
                }
                "re" if path.points.is_empty() => {
                    let [x, y, w, h] = numbers(args)?;
                    if w == 0 || h == 0 {
                        return Err(UnsupportedProfile);
                    }
                    path = Path {
                        points: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
                        closed: true,
                    };
                }
                "h" if args.is_empty() && !path.closed && !path.points.is_empty() => {
                    path.closed = true
                }
                "n" if args.is_empty() && path.points.is_empty() => {}
                "f" | "F" | "f*" | "S" | "s" | "B" | "B*" | "b" | "b*" if args.is_empty() => {
                    let (vector, bounds) = Vector::new(
                        std::mem::take(&mut path),
                        op,
                        graphics,
                        &mut self.draw_bytes,
                    )?;
                    self.add(
                        paints,
                        clip,
                        Paint {
                            bounds,
                            source: Some((id, operation)),
                            kind: PaintKind::Vector(vector),
                        },
                    )?;
                }
                _ if !path.points.is_empty() => return Err(UnsupportedProfile),
                "q" if args.is_empty() && saved.len() < 32 => saved.push(graphics),
                "Q" if args.is_empty() => graphics = saved.pop().ok_or(UnsupportedProfile)?,
                "cm" => graphics.matrix = graphics.matrix.compose(Matrix::new(numbers(args)?)?)?,
                "w" => {
                    let [width] = numbers(args)?;
                    if !(1..=1_000 * POINT).contains(&width) {
                        return Err(UnsupportedProfile);
                    }
                    graphics.width = width;
                }
                "BT" if args.is_empty() => text = Some(TextState::default()),
                "Do" => {
                    let object = resource(document, resources, "XObject", operand_name(args)?)?;
                    self.assets.insert(object);
                    let stream = document.stream(object)?;
                    match name(&stream.dict, "Subtype") {
                        Some("Image") => {
                            let image_data = match self.image_cache.entry(object) {
                                std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
                                std::collections::btree_map::Entry::Vacant(entry) => {
                                    let image = ImageData::read(
                                        &stream.dict,
                                        &stream.data,
                                        &mut self.parse_work,
                                    )?;
                                    self.image_hashes.insert(image.digest);
                                    *entry.insert(image)
                                }
                            };
                            let bounds = graphics.matrix.bounds([0, 0, POINT, POINT])?;
                            let image = Image {
                                data: image_data,
                                matrix: graphics.matrix,
                                masks: Vec::new(),
                            };
                            self.add(
                                paints,
                                clip,
                                Paint {
                                    bounds,
                                    source: Some((id, operation)),
                                    kind: PaintKind::Image(image),
                                },
                            )?;
                        }
                        Some("Form") => {
                            let (matrix, nested_clip, resources) =
                                form(document, &stream.dict, resources, graphics.matrix, clip)?;
                            self.stream(
                                document,
                                object,
                                resources,
                                Graphics { matrix, ..graphics },
                                nested_clip,
                                depth + 1,
                                paints,
                            )?;
                        }
                        _ => return Err(UnsupportedProfile),
                    }
                }
                _ => return Err(UnsupportedProfile),
            }
            Ok(())
        })?;
        if !saved.is_empty() || !path.points.is_empty() || text.is_some() {
            return Err(UnsupportedProfile);
        }
        Ok(graphics)
    }
}

struct Paint<'a> {
    bounds: Rect,
    source: Option<(PdfObjectRef, usize)>,
    kind: PaintKind<'a>,
}

enum PaintKind<'a> {
    Vector(Vector),
    Text(TextPaint),
    Image(Image<'a>),
    Note(Note<'a>),
}

impl Paint<'_> {
    fn intersects_mark(&self, mark: Rect, work: &mut usize) -> VerifyResult<bool> {
        if !intersects(mark, self.bounds) {
            return Ok(false);
        }
        let PaintKind::Vector(vector) = &self.kind else {
            return Ok(true);
        };
        if let (Some((_, evenodd)), Some(fill)) = (vector.fill, &vector.fill_path)
            && fill.fill_intersects_rect(mark, evenodd, work)?
        {
            return Ok(true);
        }
        for stencil in &vector.stroke_paths {
            spend(work, stencil.edges * 5 + 1)?;
            if stencil.intersects_rect(mark) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn same_paint(&self, other: &Self, work: &mut usize) -> VerifyResult<bool> {
        if self.bounds != other.bounds {
            return Ok(false);
        }
        match (&self.kind, &other.kind) {
            (PaintKind::Vector(left), PaintKind::Vector(right)) => {
                spend(work, left.vertices.len() + 1)?;
                Ok(left.vertices == right.vertices
                    && left.closed == right.closed
                    && left.fill == right.fill
                    && left.stroke == right.stroke
                    && left.matrix == right.matrix)
            }
            (PaintKind::Text(left), PaintKind::Text(right)) => {
                spend(work, left.glyph_ids.len() + 1)?;
                Ok(left.font == right.font
                    && left.glyph_ids == right.glyph_ids
                    && left.matrix == right.matrix
                    && left.size == right.size
                    && left.origin == right.origin
                    && left.color == right.color)
            }
            (PaintKind::Image(left), PaintKind::Image(right)) => left.same_pixels(right, work),
            (PaintKind::Note(left), PaintKind::Note(right)) => {
                spend(work, left.contents.len() + 1)?;
                Ok(left.contents == right.contents)
            }
            _ => Ok(false),
        }
    }
}

struct Note<'a> {
    id: PdfObjectRef,
    contents: &'a [u8],
}

struct TextPaint {
    font: [u8; 32],
    glyph_ids: Vec<ttf_parser::GlyphId>,
    glyphs: Vec<Stencil>,
    matrix: Matrix,
    origin: Point,
    size: i64,
    color: Color,
}

#[derive(Default)]
struct Path {
    points: Vec<Point>,
    closed: bool,
}

struct Vector {
    vertices: Vec<Point>,
    matrix: Matrix,
    closed: bool,
    fill: Option<(Color, bool)>,
    stroke: Option<(Color, i64)>,
    fill_path: Option<Stencil>,
    stroke_paths: Vec<Stencil>,
}

impl Vector {
    fn new(
        path: Path,
        op: &str,
        state: Graphics,
        memory: &mut usize,
    ) -> VerifyResult<(Self, Rect)> {
        let mut points = path.points;
        let closed = path.closed || matches!(op, "s" | "b" | "b*");
        let fill = (!matches!(op, "S" | "s")).then_some((state.fill, op.ends_with('*')));
        let stroke = (!matches!(op, "f" | "F" | "f*")).then_some((state.stroke, state.width));
        if points.len() > 2 && points.first() == points.last() {
            points.pop();
            if !closed && stroke.is_some() {
                return Err(UnsupportedProfile);
            }
        }
        if points.len() < 2
            || ((fill.is_some() || closed) && points.len() < 3)
            || points
                .iter()
                .flatten()
                .any(|v| !(-MAX_COORD..=MAX_COORD).contains(v))
            || points.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(UnsupportedProfile);
        }
        spend(memory, points.len() * std::mem::size_of::<Point>())?;
        let fill_path = if fill.is_some() {
            Some(Stencil::single(&points, state.matrix, memory)?)
        } else {
            None
        };
        let stroke_paths = if stroke.is_some() {
            stroke_stencils(&points, closed, state.width, state.matrix, memory)?
        } else {
            Vec::new()
        };
        let bounds = fill_path
            .iter()
            .chain(&stroke_paths)
            .map(|stencil| stencil.bounds)
            .reduce(|[l, b, r, t], [x, y, u, v]| [l.min(x), b.min(y), r.max(u), t.max(v)])
            .filter(|&bounds| valid_rect(bounds))
            .ok_or(UnsupportedProfile)?;
        Ok((
            Self {
                vertices: points,
                matrix: state.matrix,
                closed,
                fill,
                stroke,
                fill_path,
                stroke_paths,
            },
            bounds,
        ))
    }
}

fn stroke_stencils(
    points: &[Point],
    closed: bool,
    width: i64,
    matrix: Matrix,
    memory: &mut usize,
) -> VerifyResult<Vec<Stencil>> {
    let edges = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let mut normals = Vec::with_capacity(edges);
    let mut stencils = Vec::new();
    for index in 0..edges {
        let a = points[index];
        let b = points[(index + 1) % points.len()];
        let (normal, uncertainty) = normal_enclosure(sub(b, a), width);
        normals.push((normal, uncertainty));
        stencils.push(Stencil::stroke(
            &[
                add(a, normal),
                add(b, normal),
                sub(b, normal),
                sub(a, normal),
            ],
            uncertainty,
            matrix,
            memory,
        )?);
    }
    for index in if closed {
        0..points.len()
    } else {
        1..points.len() - 1
    } {
        let previous = (index + points.len() - 1) % points.len();
        let center = points[index];
        let incoming = sub(center, points[previous]);
        let outgoing = sub(points[(index + 1) % points.len()], center);
        let turn = cross(incoming, outgoing);
        let alignment = dot(incoming, outgoing);
        if turn == 0 {
            if alignment <= 0 {
                return Err(UnsupportedProfile);
            }
            continue;
        }

        let [(a, a_error), (b, b_error)] = [normals[previous], normals[index]];
        let outside = if turn > 0 { -1 } else { 1 };
        let a = a.map(|v| v * outside);
        let b = b.map(|v| v * outside);
        let mut error = [a_error[0].max(b_error[0]), a_error[1].max(b_error[1])];
        let mut contour = vec![center, add(center, a)];
        if let Some((tip, tip_error)) =
            join_enclosure(turn, alignment, add(a, b), add(a_error, b_error))?
        {
            contour.push(add(center, tip));
            error = [error[0].max(tip_error[0]), error[1].max(tip_error[1])];
        }
        contour.push(add(center, b));
        stencils.push(Stencil::stroke(&contour, error, matrix, memory)?);
    }
    Ok(stencils)
}

fn dot(a: Point, b: Point) -> i128 {
    a.into_iter()
        .zip(b)
        .map(|(x, y)| i128::from(x) * i128::from(y))
        .sum()
}

fn normal_enclosure([dx, dy]: Point, width: i64) -> (Point, Point) {
    let length_squared = dot([dx, dy], [dx, dy]);
    let mut normal = [0; 2];
    let mut error = [0; 2];
    for (axis, delta) in [-dy, dx].into_iter().enumerate() {
        let scaled = i128::from(delta) * i128::from(width);
        let doubled = (scaled * scaled / length_squared).isqrt();
        normal[axis] = (((doubled + 1) / 2) * scaled.signum()) as i64;
        error[axis] =
            i64::from(doubled % 2 != 0 || doubled * doubled * length_squared != scaled * scaled);
    }
    (normal, error)
}

fn join_enclosure(
    turn: i128,
    alignment: i128,
    normals: Point,
    uncertainty: Point,
) -> VerifyResult<Option<(Point, Point)>> {
    let turn_squared = turn * turn;
    let alignment_squared = alignment * alignment;
    // @invariant: Beyond 90 degrees the default miter limit occurs at sin²(turn) / cos²(turn) = 99 / 2401.
    if alignment < 0 {
        let threshold =
            (alignment_squared / 2401) * 99 + ((alignment_squared % 2401) * 99 + 2400) / 2401;
        if turn_squared < threshold {
            return Ok(None);
        }
    }
    let product = turn_squared + alignment_squared;
    let floor = product.isqrt();
    let ceil = floor + i128::from(floor * floor != product);
    if floor + alignment <= 0 {
        return Err(UnsupportedProfile);
    }
    let mut tip = [0; 2];
    let mut error = [0; 2];
    let scales = [floor, ceil];
    for axis in 0..2 {
        let sum = i128::from(normals[axis]);
        let margin = i128::from(uncertainty[axis]);
        let denominator = floor + alignment;
        let rounded = (sum * floor + denominator / 2).div_euclid(denominator);
        tip[axis] = i64::try_from(rounded).map_err(|_| UnsupportedProfile)?;
        for length in scales {
            let denominator = length + alignment;
            let lower = ((sum - margin) * length).div_euclid(denominator);
            let upper = -(-(sum + margin) * length).div_euclid(denominator);
            let distance = (rounded - lower).max(upper - rounded);
            error[axis] = error[axis].max(i64::try_from(distance).map_err(|_| UnsupportedProfile)?);
        }
    }
    Ok(Some((tip, error)))
}

fn add([x, y]: Point, [u, v]: Point) -> Point {
    [x + u, y + v]
}
fn sub([x, y]: Point, [u, v]: Point) -> Point {
    [x - u, y - v]
}

fn cross([x, y]: Point, [u, v]: Point) -> i128 {
    i128::from(x) * i128::from(v) - i128::from(y) * i128::from(u)
}

fn bounds_of(points: &[Point]) -> Option<Rect> {
    let &[x, y] = points.first()?;
    Some(points.iter().fold([x, y, x, y], |[l, b, r, t], &[x, y]| {
        [l.min(x), b.min(y), r.max(x), t.max(y)]
    }))
}

struct Stencil {
    contours: Vec<Vec<Point>>,
    bounds: Rect,
    edges: usize,
    scale: i64,
    margin: Point,
    bounds_only: bool,
}

impl Stencil {
    fn single(path: &[Point], matrix: Matrix, memory: &mut usize) -> VerifyResult<Self> {
        Self::vector(path, matrix, [0; 2], false, memory)
    }

    fn stroke(
        path: &[Point],
        uncertainty: Point,
        matrix: Matrix,
        memory: &mut usize,
    ) -> VerifyResult<Self> {
        Self::vector(path, matrix, uncertainty, true, memory)
    }

    fn vector(
        path: &[Point],
        matrix: Matrix,
        [dx, dy]: Point,
        stroke: bool,
        memory: &mut usize,
    ) -> VerifyResult<Self> {
        spend(
            memory,
            std::mem::size_of::<Self>()
                + std::mem::size_of::<Vec<Point>>()
                + std::mem::size_of_val(path),
        )?;
        let [l, b, r, t] = bounds_of(path).ok_or(UnsupportedProfile)?;
        let envelope = [l - dx, b - dy, r + dx, t + dy];
        if envelope
            .iter()
            .any(|n| !(-MAX_COORD..=MAX_COORD).contains(n))
        {
            return Err(UnsupportedProfile);
        }
        let mut stencil = Self::new(vec![path.iter().map(|&p| matrix.numerator(p)).collect()])?;
        stencil.bounds = matrix.bounds(envelope)?;
        stencil.scale = POINT;
        let [a, b, c, d, _, _] = matrix.0;
        stencil.margin = [a.abs() * dx + c.abs() * dy, b.abs() * dx + d.abs() * dy];
        if stroke {
            let vertices = &stencil.contours[0];
            let turns = (0..vertices.len()).map(|i| {
                let a = vertices[i];
                let b = vertices[(i + 1) % vertices.len()];
                let c = vertices[(i + 2) % vertices.len()];
                cross(sub(b, a), sub(c, b)).signum()
            });
            stencil.bounds_only = !turns.clone().all(|t| t > 0) && !turns.clone().all(|t| t < 0);
        }
        Ok(stencil)
    }

    fn new(contours: Vec<Vec<Point>>) -> VerifyResult<Self> {
        let mut combined: Option<Rect> = None;
        let mut edges = 0;
        for contour in &contours {
            let [l, b, r, t] = bounds_of(contour).ok_or(UnsupportedProfile)?;
            combined = Some(match combined {
                Some([x, y, u, v]) => [x.min(l), y.min(b), u.max(r), v.max(t)],
                None => [l, b, r, t],
            });
            edges += contour.len();
        }
        Ok(Self {
            contours,
            bounds: combined.ok_or(UnsupportedProfile)?,
            edges,
            scale: 1,
            margin: [0; 2],
            bounds_only: false,
        })
    }

    fn intersects_rect(&self, rect: Rect) -> bool {
        if !intersects(self.bounds, rect) {
            return false;
        }
        let [x, y, u, v] = rect.map(|coordinate| coordinate * self.scale);
        let [dx, dy] = self.margin;
        let [left, bottom, right, top] = [x - dx, y - dy, u + dx, v + dy];
        if self.bounds_only {
            let [l, b, r, t] = bounds_of(&self.contours[0]).expect("nonempty stroke contour");
            return l <= right && left <= r && b <= top && bottom <= t;
        }
        let corners = [[left, bottom], [right, bottom], [right, top], [left, top]];
        corners
            .iter()
            .any(|&point| self.hits_scaled(point, 1, false))
            || self.contours.iter().any(|contour| {
                contour
                    .iter()
                    .zip(contour.iter().cycle().skip(1))
                    .take(contour.len())
                    .any(|(&a, &b)| {
                        if a[0].max(b[0]) < left
                            || a[0].min(b[0]) > right
                            || a[1].max(b[1]) < bottom
                            || a[1].min(b[1]) > top
                        {
                            return false;
                        }
                        let direction = sub(b, a);
                        let sides = corners.map(|corner| cross(direction, sub(corner, a)));
                        sides.iter().any(|&side| side >= 0) && sides.iter().any(|&side| side <= 0)
                    })
            })
    }

    fn fill_intersects_rect(
        &self,
        rect: Rect,
        evenodd: bool,
        work: &mut usize,
    ) -> VerifyResult<bool> {
        if !intersects(self.bounds, rect) {
            return Ok(false);
        }
        spend(work, self.edges * 6 + 1)?;
        let rect @ [left, bottom, right, top] = rect.map(|coordinate| coordinate * self.scale);
        if self.hits_scaled([left + right, bottom + top], 2, evenodd) {
            return Ok(true);
        }
        let segments = self.segments();
        let mut grouped = vec![false; self.edges];
        for (index, (start, end)) in segments.clone().enumerate() {
            if grouped[index] || !crosses_interior(start, end, rect) {
                continue;
            }
            spend(work, self.edges * 32 + 1)?;
            // @invariant: Coincident edges are fill boundaries only where their net winding changes coverage.
            let axis = usize::from(start[0] == end[0]);
            let direction = sub(end, start);
            let mut events = Vec::new();
            for (index, (a, b)) in segments.clone().enumerate() {
                if a != b
                    && cross(direction, sub(a, start)) == 0
                    && cross(direction, sub(b, start)) == 0
                {
                    grouped[index] = true;
                    let change = if a[axis] < b[axis] { 1 } else { -1 };
                    events.extend([(a, change), (b, -change)]);
                }
            }
            events.sort_unstable_by_key(|(point, _)| point[axis]);
            let mut winding_change = 0;
            for pair in events.windows(2) {
                let (start, change) = pair[0];
                let (end, _) = pair[1];
                winding_change += change;
                if is_filled(winding_change, evenodd) && crosses_interior(start, end, rect) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn segments(&self) -> impl Iterator<Item = (Point, Point)> + Clone + '_ {
        self.contours.iter().flat_map(|contour| {
            contour
                .iter()
                .copied()
                .zip(contour.iter().copied().cycle().skip(1))
        })
    }

    fn hits(&self, p: Point, evenodd: bool) -> bool {
        inside(self.bounds, p) && self.hits_scaled(p.map(|n| n * self.scale), 1, evenodd)
    }

    fn hits_scaled(&self, p: Point, scale: i64, evenodd: bool) -> bool {
        let mut winding = 0_i32;
        for (a, b) in self.segments() {
            let a = a.map(|v| v * scale);
            let b = b.map(|v| v * scale);
            if a[1] <= p[1] && p[1] < b[1] && cross(sub(b, a), sub(p, a)) > 0 {
                winding += 1;
            }
            if b[1] <= p[1] && p[1] < a[1] && cross(sub(b, a), sub(p, a)) < 0 {
                winding -= 1;
            }
        }
        is_filled(winding, evenodd)
    }
}

fn is_filled(winding: i32, evenodd: bool) -> bool {
    if evenodd {
        winding % 2 != 0
    } else {
        winding != 0
    }
}

fn crosses_interior(a: Point, b: Point, [left, bottom, right, top]: Rect) -> bool {
    if a[0].max(b[0]) <= left
        || a[0].min(b[0]) >= right
        || a[1].max(b[1]) <= bottom
        || a[1].min(b[1]) >= top
    {
        return false;
    }
    let direction = sub(b, a);
    let sides = [[left, bottom], [right, bottom], [right, top], [left, top]]
        .map(|corner| cross(direction, sub(corner, a)));
    sides.iter().any(|&side| side < 0) && sides.iter().any(|&side| side > 0)
}

fn transformed(matrix: Matrix, p: Point) -> VerifyResult<Point> {
    if p.into_iter()
        .any(|value| !(-MAX_COORD..=MAX_COORD).contains(&value))
    {
        return Err(UnsupportedProfile);
    }
    let out = matrix
        .numerator(p)
        .map(|n| (n + POINT / 2).div_euclid(POINT));
    out.into_iter()
        .all(|n| (-MAX_COORD..=MAX_COORD).contains(&n))
        .then_some(out)
        .ok_or(UnsupportedProfile)
}

struct Outline<'a> {
    origin: Point,
    size: i64,
    units: i64,
    matrix: Matrix,
    current: Vec<Point>,
    contours: Vec<Vec<Point>>,
    error: Option<super::verifier::NativeVerificationFailure>,
    work: &'a mut usize,
    memory: &'a mut usize,
}

impl<'a> Outline<'a> {
    fn new(
        origin: Point,
        size: i64,
        units: u16,
        matrix: Matrix,
        work: &'a mut usize,
        memory: &'a mut usize,
    ) -> Self {
        Self {
            origin,
            size,
            units: i64::from(units),
            matrix,
            current: Vec::new(),
            contours: Vec::new(),
            error: None,
            work,
            memory,
        }
    }

    fn point(&self, x: f32, y: f32) -> VerifyResult<Point> {
        let points = [x * 2.0, y * 2.0];
        if points
            .into_iter()
            .any(|v| !v.is_finite() || v.fract() != 0.0 || v.abs() > 2_000_000.0)
        {
            return Err(UnsupportedProfile);
        }
        let local = points.map(|coordinate| {
            (coordinate as i64 * self.size + self.units).div_euclid(self.units * 2)
        });
        transformed(self.matrix, add(self.origin, local))
    }

    fn push(&mut self, point: Point) -> VerifyResult<()> {
        spend(self.work, 1)?;
        spend(self.memory, std::mem::size_of::<Point>())?;
        self.current.push(point);
        Ok(())
    }

    fn record(&mut self, action: impl FnOnce(&mut Self) -> VerifyResult<()>) {
        if self.error.is_none() {
            self.error = action(self).err();
        }
    }

    fn finish(self) -> VerifyResult<Stencil> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if !self.current.is_empty() {
            return Err(UnsupportedProfile);
        }
        spend(self.memory, std::mem::size_of::<Stencil>())?;
        Stencil::new(self.contours)
    }
}

impl OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.record(|this| {
            if !this.current.is_empty() {
                return Err(UnsupportedProfile);
            }
            this.push(this.point(x, y)?)
        });
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.record(|this| this.push(this.point(x, y)?));
    }

    fn quad_to(&mut self, x: f32, y: f32, x1: f32, y1: f32) {
        self.record(|this| {
            let start = *this.current.last().ok_or(UnsupportedProfile)?;
            let control = this.point(x, y)?;
            let end = this.point(x1, y1)?;
            for step in 1..=16 {
                let rest = 16 - step;
                let point = [0, 1].map(|axis| {
                    (rest * rest * start[axis]
                        + 2 * rest * step * control[axis]
                        + step * step * end[axis]
                        + 128)
                        .div_euclid(256)
                });
                this.push(point)?;
            }
            Ok(())
        });
    }

    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.error = Some(UnsupportedProfile);
    }

    fn close(&mut self) {
        self.record(|this| {
            if this.current.is_empty() {
                return Err(UnsupportedProfile);
            }
            spend(this.memory, std::mem::size_of::<Vec<Point>>())?;
            this.contours.push(std::mem::take(&mut this.current));
            Ok(())
        });
    }
}

#[derive(Clone, Copy)]
struct ImageData<'a> {
    bytes: &'a [u8],
    width: usize,
    height: usize,
    channels: usize,
    digest: [u8; 32],
}

impl<'a> ImageData<'a> {
    fn read(dict: &Dict, bytes: &'a [u8], work: &mut usize) -> VerifyResult<Self> {
        let dimension = |key| {
            get(dict, key)?
                .as_integer()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| (1..=MAX_IMAGE_DIMENSION).contains(n))
                .ok_or(UnsupportedProfile)
        };
        let width = dimension("Width")?;
        let height = dimension("Height")?;
        if get(dict, "BitsPerComponent")?.as_integer() != Some(8)
            || dict
                .get("Interpolate")
                .is_some_and(|v| !matches!(v, PdfValue::Boolean(false)))
            || width * height > MAX_PAGE_PIXELS
        {
            return Err(UnsupportedProfile);
        }
        let channels = match name(dict, "ColorSpace") {
            Some("DeviceGray") => 1,
            Some("DeviceRGB") => 3,
            _ => return Err(UnsupportedProfile),
        };
        if bytes.len() != width * height * channels {
            return Err(Structure);
        }
        spend(work, bytes.len() / 16 + 1)?;
        Ok(Self {
            bytes,
            width,
            height,
            channels,
            digest: *blake3::hash(bytes).as_bytes(),
        })
    }
}

struct Image<'a> {
    data: ImageData<'a>,
    matrix: Matrix,
    masks: Vec<[usize; 4]>,
}

fn charge_masks(count: usize, memory: &mut usize) -> VerifyResult<()> {
    spend(
        memory,
        count
            .checked_mul(std::mem::size_of::<[usize; 4]>())
            .ok_or(Budget)?,
    )
}

impl Image<'_> {
    fn add_masks(
        &mut self,
        marks: &[Rect],
        memory: &mut usize,
        work: &mut usize,
    ) -> VerifyResult<()> {
        spend(work, marks.len())?;
        let count = marks
            .iter()
            .filter(|&&mark| self.mask(mark).is_some())
            .count();
        charge_masks(count, memory)?;
        if count != 0 {
            self.masks.try_reserve_exact(count).map_err(|_| Budget)?;
            for &mark in marks {
                if let Some(mask) = self.mask(mark) {
                    self.masks.push(mask);
                }
            }
        }
        Ok(())
    }

    fn mask(&self, mark: Rect) -> Option<[usize; 4]> {
        let [a, b, c, d, e, f] = self.matrix.0;
        let (u, v) = if a != 0 {
            (
                image_interval(mark[0], mark[2], e, a, self.data.width),
                image_interval(mark[1], mark[3], f + d, -d, self.data.height),
            )
        } else {
            (
                image_interval(mark[1], mark[3], f, b, self.data.width),
                image_interval(mark[0], mark[2], e + c, -c, self.data.height),
            )
        };
        let ((l, r), (t, b)) = (u?, v?);
        Some([l, t, r, b])
    }

    fn masked(&self, x: usize, y: usize) -> bool {
        self.masks
            .iter()
            .any(|&[l, t, r, b]| l <= x && x < r && t <= y && y < b)
    }

    fn same_pixels(&self, other: &Self, work: &mut usize) -> VerifyResult<bool> {
        let a = self.data;
        let b = other.data;
        if self.matrix != other.matrix
            || (a.width, a.height, a.channels) != (b.width, b.height, b.channels)
        {
            return Ok(false);
        }
        spend(
            work,
            (a.width * a.height)
                .checked_mul(1 + self.masks.len())
                .ok_or(Budget)?,
        )?;
        for (index, (before, after)) in a
            .bytes
            .chunks_exact(a.channels)
            .zip(b.bytes.chunks_exact(b.channels))
            .enumerate()
        {
            if self.masked(index % a.width, index / a.width) {
                if after.iter().any(|&sample| sample != 0) {
                    return Ok(false);
                }
            } else if before != after {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn at(&self, [x, y]: Point) -> Option<Pixel> {
        let [a, b, c, d, e, f] = self.matrix.0;
        let (column, row) = if a != 0 {
            (
                image_index(x, e, a, self.data.width)?,
                image_index(y, f + d, -d, self.data.height)?,
            )
        } else {
            (
                image_index(y, f, b, self.data.width)?,
                image_index(x, e + c, -c, self.data.height)?,
            )
        };
        if self.masked(column, row) {
            return Some([0; 3]);
        }
        let start = (row * self.data.width + column) * self.data.channels;
        Some(if self.data.channels == 1 {
            [self.data.bytes[start]; 3]
        } else {
            self.data.bytes[start..start + 3].try_into().ok()?
        })
    }
}

fn image_interval(
    lo: i64,
    hi: i64,
    origin: i64,
    axis: i64,
    count: usize,
) -> Option<(usize, usize)> {
    let size = axis.abs();
    let [a, b] = [(lo - origin) * axis.signum(), (hi - origin) * axis.signum()];
    let l = a.min(b).clamp(0, size);
    let r = a.max(b).clamp(0, size);
    (l < r).then(|| {
        let n = count as i64;
        (
            (l * n / size) as usize,
            ((r * n + size - 1) / size) as usize,
        )
    })
}

fn image_index(at: i64, origin: i64, axis: i64, count: usize) -> Option<usize> {
    let distance = (at - origin) * axis.signum();
    let size = axis.abs();
    (0..size)
        .contains(&distance)
        .then(|| (distance * count as i64 / size) as usize)
}

struct Grid {
    geometry: NativeRedactionGeometry,
    width: usize,
    height: usize,
}

impl Grid {
    fn new(geometry: NativeRedactionGeometry, cumulative: &mut usize) -> VerifyResult<Self> {
        let [l, b, r, t] = geometry.crop_box;
        let [w, h] = if matches!(geometry.rotation, 90 | 270) {
            [t - b, r - l]
        } else {
            [r - l, t - b]
        };
        let width = usize::try_from((w + PITCH - 1) / PITCH).map_err(|_| Budget)?;
        let height = usize::try_from((h + PITCH - 1) / PITCH).map_err(|_| Budget)?;
        let count = width
            .checked_mul(height)
            .filter(|n| (1..=MAX_PAGE_PIXELS).contains(n))
            .ok_or(Budget)?;
        spend(cumulative, count)?;
        Ok(Self {
            geometry,
            width,
            height,
        })
    }

    fn rotate(&self, [x, y]: Point) -> Point {
        let [l, b, r, t] = self.geometry.crop_box;
        match self.geometry.rotation {
            90 => [y - b, r - x],
            180 => [r - x, t - y],
            270 => [t - y, x - l],
            _ => [x - l, y - b],
        }
    }

    fn unrotate(&self, [x, y]: Point) -> Point {
        let [l, b, r, t] = self.geometry.crop_box;
        match self.geometry.rotation {
            90 => [r - y, b + x],
            180 => [r - x, t - y],
            270 => [l + y, t - x],
            _ => [l + x, b + y],
        }
    }

    fn draw(
        &self,
        paints: &[Paint<'_>],
        sample: Point,
        work: &mut usize,
    ) -> VerifyResult<Vec<Pixel>> {
        let count = self.width * self.height;
        spend(work, count)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count).map_err(|_| Budget)?;
        pixels.resize(count, [255; 3]);
        for paint in paints {
            match &paint.kind {
                PaintKind::Text(text) => {
                    let color = text.color.rgb();
                    for glyph in &text.glyphs {
                        self.paint(
                            &mut pixels,
                            glyph.bounds,
                            sample,
                            glyph.edges + 1,
                            work,
                            |p| glyph.hits(p, false).then_some(color),
                        )?;
                    }
                }
                PaintKind::Vector(vector) => {
                    if let (Some((color, evenodd)), Some(stencil)) =
                        (vector.fill, &vector.fill_path)
                    {
                        let color = color.rgb();
                        self.paint(
                            &mut pixels,
                            stencil.bounds,
                            sample,
                            stencil.edges + 1,
                            work,
                            |p| stencil.hits(p, evenodd).then_some(color),
                        )?;
                    }
                    if let Some((color, _)) = vector.stroke {
                        let color = color.rgb();
                        for stencil in &vector.stroke_paths {
                            self.paint(
                                &mut pixels,
                                stencil.bounds,
                                sample,
                                stencil.edges + 1,
                                work,
                                |p| stencil.hits(p, false).then_some(color),
                            )?;
                        }
                    }
                }
                PaintKind::Image(image) => self.paint(
                    &mut pixels,
                    paint.bounds,
                    sample,
                    image.masks.len() + 1,
                    work,
                    |p| image.at(p),
                )?,
                PaintKind::Note(_) => {
                    self.paint(&mut pixels, paint.bounds, sample, 1, work, |p| {
                        let [l, b, r, t] = paint.bounds;
                        let inset = ((r - l).min(t - b) / 8).min(POINT);
                        Some(if inside([l + inset, b + inset, r - inset, t - inset], p) {
                            [255, 220, 64]
                        } else {
                            [40; 3]
                        })
                    })?
                }
            }
        }
        Ok(pixels)
    }

    fn paint(
        &self,
        pixels: &mut [Pixel],
        bounds: Rect,
        sample: Point,
        cost: usize,
        work: &mut usize,
        mut value: impl FnMut(Point) -> Option<Pixel>,
    ) -> VerifyResult<()> {
        let [l, b, r, t] = bounds;
        let [x, y] = self.rotate([l, b]);
        let [u, v] = self.rotate([r, t]);
        let left = (x.min(u).div_euclid(PITCH)).clamp(0, self.width as i64) as usize;
        let right = ((x.max(u) + PITCH - 1).div_euclid(PITCH)).clamp(0, self.width as i64) as usize;
        let bottom = (y.min(v).div_euclid(PITCH)).clamp(0, self.height as i64) as usize;
        let top = ((y.max(v) + PITCH - 1).div_euclid(PITCH)).clamp(0, self.height as i64) as usize;
        spend(
            work,
            ((right - left) * (top - bottom))
                .checked_mul(cost)
                .and_then(|cost| cost.checked_add(1))
                .ok_or(Budget)?,
        )?;
        for row in bottom..top {
            for col in left..right {
                let logical = [
                    col as i64 * PITCH + sample[0],
                    row as i64 * PITCH + sample[1],
                ];
                let point = self.unrotate(logical);
                if inside(bounds, point)
                    && inside(self.geometry.crop_box, point)
                    && let Some(color) = value(point)
                {
                    pixels[row * self.width + col] = color;
                }
            }
        }
        Ok(())
    }
}

fn inside([l, b, r, t]: Rect, [x, y]: Point) -> bool {
    l <= x && x < r && b <= y && y < t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_vector_stencil_keeps_exact_edge_coordinates() {
        let path = [[1, 1], [3, 1], [3, 3], [1, 3]];
        let matrix = Matrix::new([501, 0, 0, 501, 0, 0]).unwrap();
        let stencil = Stencil::single(&path, matrix, &mut 4096).unwrap();
        assert_eq!(stencil.bounds, [0, 0, 2, 2]);
        assert!(
            stencil
                .fill_intersects_rect([0, 0, 1, 1], false, &mut 4096)
                .unwrap()
        );
        assert!(stencil.hits([1, 1], false));
        assert!(!stencil.hits([0, 0], false));
    }

    #[test]
    fn stroke_coverage_includes_the_default_miter() {
        let vertices = [[10_000, 10_000], [20_000, 10_000], [20_000, 20_000]];
        let stencils =
            stroke_stencils(&vertices, false, 2_000, Matrix::IDENTITY, &mut 4096).unwrap();
        let corner = [20_200, 9_200, 20_800, 9_800];
        assert_eq!(stencils.len(), 3);
        assert!(!stencils[0].intersects_rect(corner));
        assert!(!stencils[1].intersects_rect(corner));
        assert!(stencils[2].intersects_rect(corner));
        assert!(stencils[2].hits([20_500, 9_500], false));
    }

    #[test]
    fn mask_storage_is_charged_before_reservation() {
        let mut image = Image {
            data: ImageData {
                bytes: &[127],
                width: 1,
                height: 1,
                channels: 1,
                digest: [0; 32],
            },
            matrix: Matrix::IDENTITY,
            masks: Vec::new(),
        };
        let bytes = std::mem::size_of::<[usize; 4]>();
        let marks = [[0, 0, 500, 500], [1_000, 0, 2_000, 500]];
        assert_eq!(
            image.add_masks(&marks, &mut (bytes - 1), &mut 10),
            Err(Budget)
        );
        assert_eq!(image.masks.capacity(), 0);
        let mut memory = bytes;
        image.add_masks(&marks, &mut memory, &mut 10).unwrap();
        assert_eq!(image.masks, [[0, 0, 1, 1]]);
        assert_eq!(memory, 0);
        assert_eq!(
            charge_masks(usize::MAX, &mut usize::MAX.clone()),
            Err(Budget)
        );
    }

    #[test]
    fn pixel_caps_and_cumulative_budget_are_checked() {
        let square = [0, 0, 2_000_000, 2_000_000];
        let mut geometry = NativeRedactionGeometry {
            media_box: square,
            crop_box: square,
            rotation: 90,
        };
        let mut remaining = MAX_PAGE_PIXELS;
        assert!(Grid::new(geometry, &mut remaining).is_ok());
        assert_eq!(remaining, 0);
        assert_eq!(Grid::new(geometry, &mut remaining).err(), Some(Budget));
        geometry.crop_box[2] += 1;
        assert_eq!(
            Grid::new(geometry, &mut MAX_TOTAL_PIXELS.clone()).err(),
            Some(Budget)
        );
    }

    #[test]
    fn grid_renders_full_rotated_page_and_charges_work() {
        let crop_box = [1_000, 2_000, 3_000, 3_000];
        let geometry = NativeRedactionGeometry {
            media_box: crop_box,
            crop_box,
            rotation: 90,
        };
        let grid = Grid::new(geometry, &mut MAX_TOTAL_PIXELS.clone()).unwrap();
        assert_eq!([grid.width, grid.height], [2, 4]);
        let (vector, bounds) = Vector::new(
            Path {
                points: vec![
                    [1_000, 2_000],
                    [2_000, 2_000],
                    [2_000, 3_000],
                    [1_000, 3_000],
                ],
                closed: true,
            },
            "f",
            Graphics::default(),
            &mut MAX_DRAW_BYTES.clone(),
        )
        .unwrap();
        let paint = Paint {
            bounds,
            source: None,
            kind: PaintKind::Vector(vector),
        };
        assert_eq!(
            grid.draw(
                std::slice::from_ref(&paint),
                SAMPLE_OFFSETS[0],
                &mut MAX_RASTER_WORK.clone()
            )
            .unwrap(),
            [
                [255; 3], [255; 3], [255; 3], [255; 3], [0; 3], [0; 3], [0; 3], [0; 3]
            ],
        );
        assert_eq!(grid.draw(&[paint], SAMPLE_OFFSETS[0], &mut 8), Err(Budget));
    }

    #[test]
    fn embedded_font_outline_rasterizes_curved_and_composite_glyphs() {
        let bytes = include_bytes!("../../../data/NotoSans-Regular.ttf");
        let face = ttf_parser::Face::parse(bytes, 0).unwrap();
        for character in ['A', 'B', 'Ї'] {
            let mut work = 20_000;
            let mut memory = 32_768;
            let mut outline = Outline::new(
                [5000; 2],
                12_000,
                face.units_per_em(),
                Matrix::IDENTITY,
                &mut work,
                &mut memory,
            );
            face.outline_glyph(face.glyph_index(character).unwrap(), &mut outline)
                .unwrap();
            let stencil = outline.finish().unwrap();
            assert!(stencil.edges > 5);
            assert!(valid_rect(stencil.bounds));
        }
    }

    #[test]
    fn sample_coverage_is_half_open_and_handles_reversed_axes() {
        assert_eq!(image_interval(0, 10, 0, 20, 2), Some((0, 1)));
        assert_eq!(image_interval(0, 10, 20, -20, 2), Some((1, 2)));
        assert_eq!(image_interval(10, 11, 0, 20, 2), Some((1, 2)));
        assert_eq!(image_interval(20, 21, 0, 20, 2), None);
    }

    #[test]
    fn fill_overlap_respects_coincident_edges_and_work_limits() {
        let rectangle = [[0, 0], [2_000, 0], [2_000, 2_000], [0, 2_000]];
        let outline = rectangle.repeat(2);
        let fill =
            Stencil::single(&outline, Matrix::IDENTITY, &mut MAX_DRAW_BYTES.clone()).unwrap();
        let crossing = [-1_000, 500, 100, 1_500];
        assert!(
            fill.fill_intersects_rect(crossing, false, &mut 1_000)
                .unwrap()
        );
        assert!(
            !fill
                .fill_intersects_rect(crossing, true, &mut 1_000)
                .unwrap()
        );
        assert_eq!(
            fill.fill_intersects_rect(crossing, true, &mut 0),
            Err(Budget)
        );
    }
}
