//! Read-only scene projection for the controlled PDF subset.
//!
//! The interpreter consumes the lazy reader and produces fixed-point, typed
//! elements.  It intentionally does not produce FlowDocument nodes and never
//! invokes a PDF action, URL, file, or external resource.

use std::{
    borrow::Cow,
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use serde::{Deserialize, Serialize};

use crate::layout::{LayoutRect, LayoutUnit};

use super::reader::{
    AnnotationValue, PageResources, PdfObjectRef, PdfPageRecord, PdfReadDiagnostic,
    PdfReadDiagnosticCode, PdfReadError, PdfReadReport, PdfReader, PdfReaderLimits, PdfStream,
    PdfValue, charge_scene_workspace, decode_pdf_text_string, map_widget_label_with_workspace,
    read_scene_name, report_active_content, reserve_scene_vec, resolve_annotation_value,
    scene_name_label, single_filter,
};

use super::native::{NativeNoteId, NativePageGeometry, NativePageId, NativeTextPage};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativePdfNote {
    pub id: NativeNoteId,
    pub object: PdfObjectRef,
    pub contents: String,
    pub rect: [i64; 4],
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeReadOnlyAnnotation {
    Text,
    Appearance,
    Link,
    Widget,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativePdfAnnotation {
    PlainNote(NativePdfNote),
    ReadOnly {
        object: Option<PdfObjectRef>,
        profile: NativeReadOnlyAnnotation,
        rect: Option<[LayoutUnit; 4]>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativePdfNotePage {
    pub id: NativePageId,
    pub object: PdfObjectRef,
    pub geometry: Option<NativePageGeometry>,
    pub can_add_note: bool,
    pub annotations: Vec<NativePdfAnnotation>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativePdfNoteScene {
    pub output_hash: [u8; 32],
    pub scene_hash: [u8; 32],
    pub pages: Vec<NativePdfNotePage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub text_pages: Vec<NativeTextPage>,
    pub object_pages: Vec<super::native::NativeObjectPage>,
}

impl NativePdfNoteScene {
    pub(super) fn new(
        output_hash: [u8; 32],
        pages: Vec<NativePdfNotePage>,
        text_pages: Vec<NativeTextPage>,
        object_pages: Vec<super::native::NativeObjectPage>,
        work: &mut usize,
    ) -> Result<Self, super::native::NativeSessionError> {
        use super::native::NativeSessionError::{self, Budget, Postcondition};

        struct HashWriter<'a> {
            hash: blake3::Hasher,
            work: &'a mut usize,
            bytes_written: usize,
        }

        impl HashWriter<'_> {
            fn update(&mut self, bytes: &[u8]) -> Result<(), NativeSessionError> {
                let bytes_written = self.bytes_written.checked_add(bytes.len()).ok_or(Budget)?;
                // @invariant: Meter the canonical byte stream independently of serializer write boundaries.
                let cost = bytes_written.div_ceil(16) - self.bytes_written.div_ceil(16);
                *self.work = self.work.checked_sub(cost).ok_or(Budget)?;
                self.bytes_written = bytes_written;
                self.hash.update(bytes);
                Ok(())
            }

            fn encode(&mut self, value: &impl Serialize) -> Result<(), NativeSessionError> {
                serde_json::to_writer(self, value).map_err(|error| {
                    if error.io_error_kind() == Some(std::io::ErrorKind::FileTooLarge) {
                        Budget
                    } else {
                        Postcondition
                    }
                })
            }
        }

        impl std::io::Write for HashWriter<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.update(bytes)
                    .map_err(|_| std::io::ErrorKind::FileTooLarge)?;
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut writer = HashWriter {
            hash: blake3::Hasher::new(),
            work,
            bytes_written: 0,
        };
        writer.update(b"flowpdf:accepted-note-scene:v2")?;
        writer.update(&output_hash)?;
        for page in &pages {
            writer.encode(&(page.object, page.geometry, page.can_add_note))?;
            for annotation in &page.annotations {
                match annotation {
                    NativePdfAnnotation::PlainNote(note) => {
                        writer.encode(&(note.object, note.rect, &note.contents))?
                    }
                    NativePdfAnnotation::ReadOnly {
                        object,
                        profile,
                        rect,
                    } => writer.encode(&(object, profile, rect))?,
                }
            }
        }
        if !text_pages.is_empty() {
            writer.update(b"flowpdf:native-text-pages:v1")?;
            writer.encode(&text_pages)?;
        }
        writer.update(b"flowpdf:native-object-pages:v1")?;
        writer.encode(&object_pages)?;
        Ok(Self {
            output_hash,
            scene_hash: *writer.hash.finalize().as_bytes(),
            pages,
            text_pages,
            object_pages,
        })
    }
}

pub const PDF_SCENE_SCHEMA_VERSION: u32 = 1;

/// Page window requested from a reader. The reader remains lazy outside this
/// window and the request is always capped by the reader's limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfSceneRequest {
    pub first_page: u32,
    pub page_count: u32,
}

impl Default for PdfSceneRequest {
    fn default() -> Self {
        Self {
            first_page: 0,
            page_count: 1,
        }
    }
}

/// Provenance retained on every scene element and glyph where the source
/// supplied enough information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfSourceProvenance {
    pub object_ref: PdfObjectRef,
    pub stream_offset: u32,
    pub operator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_end: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operand_span: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation_path: Vec<u32>,
}

impl PdfSourceProvenance {
    fn heap_bytes(&self) -> usize {
        self.operator.as_ref().map_or(0, String::capacity)
            + self.invocation_path.capacity() * size_of::<u32>()
    }

    fn clone_heap_bytes(&self) -> usize {
        self.operator.as_deref().map_or(0, str::len) + size_of_val(self.invocation_path.as_slice())
    }
}

/// Whether a text code was mapped to Unicode by the source PDF.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PdfTextMapping {
    ToUnicode,
    SimpleEncoding,
    Missing,
}

/// One mapped or unmapped glyph in a text element.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfGlyph {
    pub code: u32,
    pub unicode: Option<String>,
    pub rect: LayoutRect,
    pub advance: LayoutUnit,
    pub provenance: PdfSourceProvenance,
}

/// A run of text with glyph-level geometry and mapping status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfTextElement {
    pub text: String,
    pub rect: LayoutRect,
    pub font_name: Option<String>,
    pub mapping: PdfTextMapping,
    pub glyphs: Vec<PdfGlyph>,
    pub provenance: PdfSourceProvenance,
}

/// Fixed-point path commands after the content-stream CTM has been applied.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PdfPathCommand {
    MoveTo {
        x: LayoutUnit,
        y: LayoutUnit,
    },
    LineTo {
        x: LayoutUnit,
        y: LayoutUnit,
    },
    CurveTo {
        x1: LayoutUnit,
        y1: LayoutUnit,
        x2: LayoutUnit,
        y2: LayoutUnit,
        x3: LayoutUnit,
        y3: LayoutUnit,
    },
    ClosePath,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfPathElement {
    pub commands: Vec<PdfPathCommand>,
    pub rect: LayoutRect,
    pub stroke: bool,
    pub fill: bool,
    pub provenance: PdfSourceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfClipElement {
    pub commands: Vec<PdfPathCommand>,
    pub even_odd: bool,
    pub rect: LayoutRect,
    pub provenance: PdfSourceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfImageElement {
    pub width: u32,
    pub height: u32,
    pub media_type: String,
    pub filter: String,
    pub data_hex: String,
    pub rect: LayoutRect,
    pub provenance: PdfSourceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PdfAnnotationKind {
    Link,
    Widget,
    Supported,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfLinkElement {
    pub destination_page: Option<u32>,
    pub rect: LayoutRect,
    pub internal: bool,
    pub provenance: PdfSourceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfFormElement {
    pub field_type: Option<String>,
    pub name: Option<String>,
    pub value: Option<String>,
    pub rect: LayoutRect,
    pub provenance: PdfSourceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfAnnotationElement {
    pub subtype: Option<String>,
    pub kind: PdfAnnotationKind,
    pub rect: LayoutRect,
    pub provenance: PdfSourceProvenance,
}

/// A scene element is derived from one page/content object and is never an
/// editor mutation or a callback-bearing value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PdfSceneElement {
    Text { value: PdfTextElement },
    Path { value: PdfPathElement },
    Clip { value: PdfClipElement },
    Image { value: PdfImageElement },
    Form { value: PdfFormElement },
    Link { value: PdfLinkElement },
    Annotation { value: PdfAnnotationElement },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfScenePage {
    pub page_index: u32,
    pub source_object: PdfObjectRef,
    pub bounds: LayoutRect,
    pub elements: Vec<PdfSceneElement>,
    pub report: PdfReadReport,
    pub partial: bool,
}

/// Complete derived scene result for one bounded page window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfScene {
    pub schema_version: u32,
    pub source_hash: String,
    pub page_count: u32,
    pub first_page: u32,
    pub pages: Vec<PdfScenePage>,
    pub report: PdfReadReport,
    pub result_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Matrix {
    a: LayoutUnit,
    b: LayoutUnit,
    c: LayoutUnit,
    d: LayoutUnit,
    e: LayoutUnit,
    f: LayoutUnit,
}

impl Matrix {
    const IDENTITY: Self = Self {
        a: LayoutUnit::from_raw(LayoutUnit::UNITS_PER_POINT),
        b: LayoutUnit::from_raw(0),
        c: LayoutUnit::from_raw(0),
        d: LayoutUnit::from_raw(LayoutUnit::UNITS_PER_POINT),
        e: LayoutUnit::from_raw(0),
        f: LayoutUnit::from_raw(0),
    };

    fn from_pdf_array(value: &PdfValue) -> Result<Self, PdfReadError> {
        let [a, b, c, d, e, f] = value.as_array().ok_or(PdfReadError::Syntax)? else {
            return Err(PdfReadError::Syntax);
        };
        Ok(Self {
            a: a.as_number().ok_or(PdfReadError::Syntax)?,
            b: b.as_number().ok_or(PdfReadError::Syntax)?,
            c: c.as_number().ok_or(PdfReadError::Syntax)?,
            d: d.as_number().ok_or(PdfReadError::Syntax)?,
            e: e.as_number().ok_or(PdfReadError::Syntax)?,
            f: f.as_number().ok_or(PdfReadError::Syntax)?,
        })
    }

    fn multiply(self, other: Self) -> Result<Self, PdfReadError> {
        Ok(Self {
            a: mul(self.a, other.a)?.checked_add(mul(self.c, other.b)?)?,
            b: mul(self.b, other.a)?.checked_add(mul(self.d, other.b)?)?,
            c: mul(self.a, other.c)?.checked_add(mul(self.c, other.d)?)?,
            d: mul(self.b, other.c)?.checked_add(mul(self.d, other.d)?)?,
            e: mul(self.a, other.e)?
                .checked_add(mul(self.c, other.f)?)?
                .checked_add(self.e)?,
            f: mul(self.b, other.e)?
                .checked_add(mul(self.d, other.f)?)?
                .checked_add(self.f)?,
        })
    }

    fn point(self, x: LayoutUnit, y: LayoutUnit) -> Result<(LayoutUnit, LayoutUnit), PdfReadError> {
        Ok((
            mul(self.a, x)?
                .checked_add(mul(self.c, y)?)?
                .checked_add(self.e)?,
            mul(self.b, x)?
                .checked_add(mul(self.d, y)?)?
                .checked_add(self.f)?,
        ))
    }
}

fn mul(left: LayoutUnit, right: LayoutUnit) -> Result<LayoutUnit, PdfReadError> {
    let product = i128::from(left.raw())
        .checked_mul(i128::from(right.raw()))
        .ok_or(PdfReadError::NumericLimit)?;
    let rounded = if product.is_negative() {
        -((-product + 32) / 64)
    } else {
        (product + 32) / 64
    };
    i64::try_from(rounded)
        .map(LayoutUnit::from_raw)
        .map_err(|_| PdfReadError::NumericLimit)
}

fn scale_font_width(
    width_thousandths: LayoutUnit,
    font_size: LayoutUnit,
) -> Result<LayoutUnit, PdfReadError> {
    let product = i128::from(width_thousandths.raw())
        .checked_mul(i128::from(font_size.raw()))
        .ok_or(PdfReadError::NumericLimit)?;
    let value = product / 1_000;
    i64::try_from(value)
        .map(LayoutUnit::from_raw)
        .map_err(|_| PdfReadError::NumericLimit)
}

fn scale_percent(value: LayoutUnit) -> Result<LayoutUnit, PdfReadError> {
    let product = i128::from(value.raw())
        .checked_mul(i128::from(LayoutUnit::UNITS_PER_POINT))
        .ok_or(PdfReadError::NumericLimit)?;
    i64::try_from(product / 100)
        .map(LayoutUnit::from_raw)
        .map_err(|_| PdfReadError::NumericLimit)
}

#[derive(Debug, Clone, Copy)]
struct SceneClip {
    bounds: LayoutRect,
    rectangular: bool,
}

impl SceneClip {
    fn intersect(self, bounds: LayoutRect, rectangular: bool) -> Result<Self, PdfReadError> {
        let x = self.bounds.x.max(bounds.x);
        let y = self.bounds.y.max(bounds.y);
        let right = self
            .bounds
            .x
            .checked_add(self.bounds.width)?
            .min(bounds.x.checked_add(bounds.width)?);
        let top = self
            .bounds
            .y
            .checked_add(self.bounds.height)?
            .min(bounds.y.checked_add(bounds.height)?);
        Ok(Self {
            bounds: LayoutRect {
                x,
                y,
                width: right.max(x).checked_sub(x)?,
                height: top.max(y).checked_sub(y)?,
            },
            rectangular: self.rectangular && rectangular,
        })
    }

    fn is_empty(self) -> bool {
        self.bounds.width.raw() <= 0 || self.bounds.height.raw() <= 0
    }

    fn contains(self, rect: LayoutRect) -> Result<bool, PdfReadError> {
        Ok(self.bounds.x <= rect.x
            && self.bounds.y <= rect.y
            && self.bounds.x.checked_add(self.bounds.width)? >= rect.x.checked_add(rect.width)?
            && self.bounds.y.checked_add(self.bounds.height)? >= rect.y.checked_add(rect.height)?)
    }
}

#[derive(Debug, Clone, Copy)]
struct PaintBounds([f64; 4]);

impl PaintBounds {
    fn union(self, other: Self) -> Self {
        let [left, bottom, right, top] = self.0;
        let [x0, y0, x1, y1] = other.0;
        Self([left.min(x0), bottom.min(y0), right.max(x1), top.max(y1)])
    }

    fn intersect(self, other: Self) -> Self {
        let [left, bottom, right, top] = self.0;
        let [x0, y0, x1, y1] = other.0;
        Self([left.max(x0), bottom.max(y0), right.min(x1), top.min(y1)])
    }

    fn contains(self, other: Self) -> bool {
        let [left, bottom, right, top] = self.0;
        let [x0, y0, x1, y1] = other.0;
        left <= x0 && bottom <= y0 && right >= x1 && top >= y1
    }

    fn expanded(self, margin: f64) -> Option<Self> {
        let [left, bottom, right, top] = self.0;
        let edges = [
            (left - margin).next_down(),
            (bottom - margin).next_down(),
            (right + margin).next_up(),
            (top + margin).next_up(),
        ];
        edges
            .iter()
            .all(|edge| edge.is_finite())
            .then_some(Self(edges))
    }

    fn is_empty(self) -> bool {
        let [left, bottom, right, top] = self.0;
        left >= right || bottom >= top
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PaintMatrix([f64; 6]);

impl PaintMatrix {
    fn is_axis_aligned(self) -> bool {
        let [a, b, c, d, ..] = self.0;
        (b == 0. && c == 0.) || (a == 0. && d == 0.)
    }

    fn supports_image_rect(self) -> bool {
        let [a, b, c, d, ..] = self.0;
        a > 0. && d > 0. && b == 0. && c == 0.
    }

    fn multiply(self, other: Self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = other.0;
        let values = [
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ];
        values
            .iter()
            .all(|value| value.is_finite())
            .then_some(Self(values))
    }

    fn pdf_numbers<const N: usize>(value: &PdfValue) -> Option<[f64; N]> {
        let values: &[PdfValue; N] = value.as_array()?.try_into().ok()?;
        let mut numbers = [0.; N];
        for (number, value) in numbers.iter_mut().zip(values) {
            *number = value.as_precise_number()?;
        }
        Some(numbers)
    }

    fn bounds(self, [x, y, right, top]: [f64; 4]) -> Option<PaintBounds> {
        let [a, b, c, d, e, f] = self.0;
        let point = |x, y| [a * x + c * y + e, b * x + d * y + f];
        let corners = [
            point(x, y),
            point(right, y),
            point(x, top),
            point(right, top),
        ];
        if !corners.iter().flatten().all(|value| value.is_finite()) {
            return None;
        }
        let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = corners;
        Some(PaintBounds([
            x0.min(x1).min(x2).min(x3),
            y0.min(y1).min(y2).min(y3),
            x0.max(x1).max(x2).max(x3),
            y0.max(y1).max(y2).max(y3),
        ]))
    }
}

#[derive(Debug, Clone)]
struct GraphicsState {
    ctm: Matrix,
    paint_ctm: Option<PaintMatrix>,
    paint_clip: PaintBounds,
    path_paint_bounds: Option<PaintBounds>,
    text_matrix: Matrix,
    line_matrix: Matrix,
    font: Option<SelectedFont>,
    font_size: LayoutUnit,
    leading: LayoutUnit,
    char_spacing: LayoutUnit,
    word_spacing: LayoutUnit,
    horizontal_scale: LayoutUnit,
    rise: LayoutUnit,
    in_text: bool,
    path: Vec<RawPathCommand>,
    current_point: Option<(LayoutUnit, LayoutUnit)>,
    subpath_start: Option<(LayoutUnit, LayoutUnit)>,
    clip: SceneClip,
    line_width: Option<f64>,
    miter_limit: Option<f64>,
    solid_stroke: bool,
    path_ctm_changed: bool,
    pending_clip: Option<PendingClip>,
}

impl GraphicsState {
    fn new(bounds: LayoutRect, paint_clip: PaintBounds) -> Self {
        Self {
            ctm: Matrix::IDENTITY,
            paint_ctm: Some(PaintMatrix([1., 0., 0., 1., 0., 0.])),
            paint_clip,
            path_paint_bounds: None,
            text_matrix: Matrix::IDENTITY,
            line_matrix: Matrix::IDENTITY,
            font: None,
            font_size: LayoutUnit::from_raw(10 * 64),
            leading: LayoutUnit::from_raw(12 * 64),
            char_spacing: LayoutUnit::from_raw(0),
            word_spacing: LayoutUnit::from_raw(0),
            horizontal_scale: Matrix::IDENTITY.a,
            rise: LayoutUnit::from_raw(0),
            in_text: false,
            path: Vec::new(),
            current_point: None,
            subpath_start: None,
            clip: SceneClip {
                bounds,
                rectangular: true,
            },
            line_width: Some(1.),
            miter_limit: Some(10.),
            solid_stroke: true,
            path_ctm_changed: false,
            pending_clip: None,
        }
    }

    fn extend_path_paint_bounds(&mut self, edges: [f64; 4]) {
        let bounds = self.paint_ctm.and_then(|matrix| matrix.bounds(edges));
        self.path_paint_bounds = if self.path.is_empty() {
            bounds
        } else {
            self.path_paint_bounds
                .zip(bounds)
                .map(|(path, next)| path.union(next))
        };
    }

    fn heap_bytes(&self) -> usize {
        self.path.capacity() * size_of::<RawPathCommand>()
            + self
                .pending_clip
                .as_ref()
                .map_or(0, PendingClip::heap_bytes)
    }

    fn clone_heap_bytes(&self) -> usize {
        size_of_val(self.path.as_slice())
            + self
                .pending_clip
                .as_ref()
                .map_or(0, PendingClip::clone_heap_bytes)
    }
}

#[derive(Debug, Clone)]
struct PendingClip {
    commands: Vec<RawPathCommand>,
    even_odd: bool,
    provenance: PdfSourceProvenance,
    paint_bounds: Option<PaintBounds>,
}

impl PendingClip {
    fn heap_bytes(&self) -> usize {
        self.commands.capacity() * size_of::<RawPathCommand>() + self.provenance.heap_bytes()
    }

    fn clone_heap_bytes(&self) -> usize {
        size_of_val(self.commands.as_slice()) + self.provenance.clone_heap_bytes()
    }
}

#[derive(Debug, Clone)]
enum RawPathCommand {
    MoveTo(LayoutUnit, LayoutUnit, [SceneDecimal; 2]),
    LineTo(LayoutUnit, LayoutUnit, [SceneDecimal; 2]),
    CurveTo(
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
        [[SceneDecimal; 2]; 3],
    ),
    Close,
}

#[derive(Debug)]
struct FontMap {
    base_name: Option<String>,
    to_unicode: BTreeMap<Vec<u8>, String>,
    simple_encoding: bool,
    widths: BTreeMap<u8, LayoutUnit>,
    default_width: LayoutUnit,
}

impl FontMap {
    fn glyphs<'a>(&'a self, mut bytes: &'a [u8]) -> impl Iterator<Item = (u8, Option<&'a str>)> {
        std::iter::from_fn(move || {
            if bytes.is_empty() {
                return None;
            }
            let (code_bytes, code) = next_code(bytes, &self.to_unicode);
            bytes = &bytes[code_bytes.len()..];
            let unicode = self.to_unicode.get(code_bytes).map(String::as_str);
            let unicode = if unicode.is_none() && self.simple_encoding && code <= 0x7F {
                std::str::from_utf8(code_bytes).ok()
            } else {
                unicode
            };
            Some((code, unicode))
        })
    }
}

#[derive(Debug, Clone)]
struct SelectedFont {
    map: Rc<FontMap>,
    fallback_name: Option<Rc<str>>,
}

const fn scene_tree_entry_bytes<T>() -> usize {
    // @invariant: Each entry reserves space for a full tree node, including child pointers.
    128 + 12 * size_of::<T>()
}

#[derive(Debug, Clone)]
pub(super) enum ContentToken {
    Number((LayoutUnit, SceneDecimal)),
    Integer(i64),
    Name(String),
    String(Vec<u8>),
    Array(Vec<ContentToken>),
    Word(String),
}

#[derive(Debug, Clone, Copy)]
pub(super) enum SceneDecimal {
    Exact { coefficient: i64, scale: u32 },
    Approximate(f64),
}

impl SceneDecimal {
    fn value(self) -> f64 {
        match self {
            Self::Exact { coefficient, scale } => coefficient as f64 / 10_i128.pow(scale) as f64,
            Self::Approximate(value) => value,
        }
    }

    fn sum(self, other: Self) -> Self {
        if let (
            Self::Exact {
                coefficient: left,
                scale: left_scale,
            },
            Self::Exact {
                coefficient: right,
                scale: right_scale,
            },
        ) = (self, other)
        {
            let scale = left_scale.max(right_scale);
            let left = i128::from(left).checked_mul(10_i128.pow(scale - left_scale));
            let right = i128::from(right).checked_mul(10_i128.pow(scale - right_scale));
            if let Some(sum) = left.zip(right).and_then(|(a, b)| a.checked_add(b)) {
                return i64::try_from(sum).map_or_else(
                    |_| Self::Approximate(sum as f64 / 10_i128.pow(scale) as f64),
                    |coefficient| Self::Exact { coefficient, scale },
                );
            }
        }
        Self::Approximate(self.value() + other.value())
    }
}

#[derive(Debug)]
struct SceneBuilder<'a> {
    reader: &'a PdfReader,
    limits: &'a PdfReaderLimits,
    page_index: u32,
    page: &'a PdfPageRecord,
    field_root_counts: &'a mut Option<BTreeMap<PdfObjectRef, usize>>,
    link_work: usize,
    widget_work: usize,
    remaining_content_tokens: usize,
    budget: SceneBudget,
    elements: Vec<PdfSceneElement>,
    emit_elements: bool,
    report: PdfReadReport,
    operator_diagnostic_bytes: usize,
    fonts: BTreeMap<PdfObjectRef, Rc<FontMap>>,
    font_aliases: BTreeSet<Rc<str>>,
    font_cache_bytes: usize,
    page_refs: &'a [(PdfObjectRef, u32)],
    state: GraphicsState,
    state_stack: Vec<GraphicsState>,
    invocation_path: Vec<u32>,
}

#[derive(Default)]
struct ContentStreamEnd {
    pending_operands: bool,
    open_comment: bool,
}

impl<'a> SceneBuilder<'a> {
    fn new(
        reader: &'a PdfReader,
        page_refs: &'a [(PdfObjectRef, u32)],
        page_index: u32,
        page: &'a PdfPageRecord,
        field_root_counts: &'a mut Option<BTreeMap<PdfObjectRef, usize>>,
        budget: SceneBudget,
    ) -> Self {
        Self {
            reader,
            limits: reader.limits(),
            page_index,
            page,
            field_root_counts,
            link_work: reader.limits().max_content_tokens,
            widget_work: reader.limits().max_content_tokens,
            remaining_content_tokens: reader.limits().max_content_tokens,
            budget,
            elements: Vec::new(),
            emit_elements: true,
            report: PdfReadReport::default(),
            operator_diagnostic_bytes: 0,
            fonts: BTreeMap::new(),
            font_aliases: BTreeSet::new(),
            font_cache_bytes: 0,
            page_refs,
            state: GraphicsState::new(page.media_box, PaintBounds(page.media_edges)),
            state_stack: Vec::new(),
            invocation_path: Vec::new(),
        }
    }

    fn run(mut self) -> Result<(PdfScenePage, SceneBudget), PdfReadError> {
        if let Some(crop_box) = self.page.crop_box {
            self.state.clip = self.state.clip.intersect(crop_box, true)?;
        }
        if let Some(crop_edges) = self.page.crop_edges {
            self.state.paint_clip = self.state.paint_clip.intersect(PaintBounds(crop_edges));
        }
        self.inspect_annotations()?;
        let resource_locator = self
            .page
            .resources
            .filter(|_| !self.page.contents.is_empty());
        let resource_owner = match resource_locator {
            Some(PageResources::Direct(owner)) => Some(self.reader.scene_object(
                owner,
                0,
                self.budget.remaining_scene_bytes,
            )?),
            Some(PageResources::Indirect(reference, depth)) => Some(self.reader.scene_object(
                reference,
                depth,
                self.budget.remaining_scene_bytes,
            )?),
            None => None,
        };
        let resource_bytes = resource_owner.as_ref().map_or(0, |(_, bytes)| *bytes);
        self.budget.charge(resource_bytes)?;
        let resources = resource_owner.as_ref().and_then(|(owner, _)| {
            if matches!(resource_locator, Some(PageResources::Direct(_))) {
                owner.as_dictionary()?.get("Resources")
            } else {
                Some(owner)
            }
        });
        let mut previous = None::<(PdfObjectRef, u8, bool)>;
        let mut unverified_boundary = None;
        for (index, content_ref) in self.page.contents.iter().enumerate() {
            self.invocation_path.clear();
            self.budget.reserve_vec(&mut self.invocation_path, 1)?;
            self.invocation_path
                .push(u32::try_from(index).map_err(|_| PdfReadError::NumericLimit)?);
            match self
                .reader
                .scene_object(*content_ref, 0, self.budget.remaining_scene_bytes)
            {
                Ok((PdfValue::Stream(stream), bytes)) => {
                    let Some(&last_byte) = stream.data.last() else {
                        continue;
                    };
                    let uncertain = previous
                        .filter(|(_, byte, _)| !separated_content_streams(&[*byte], &stream.data));
                    if let Some((previous_ref, _, open_comment)) = uncertain {
                        if open_comment {
                            drop(stream);
                            self.report_partial(previous_ref);
                            break;
                        }
                        // @contract: Independent stream previews across unverified boundaries are partial.
                        unverified_boundary.get_or_insert(previous_ref);
                    }
                    self.budget.charge(bytes)?;
                    let end = self.interpret_stream(
                        &stream,
                        *content_ref,
                        0,
                        resources,
                        index + 1 < self.page.contents.len(),
                    );
                    drop(stream);
                    self.budget.release(bytes);
                    let end = match (end, uncertain) {
                        (Err(ContentParseError::OperatorSyntax), Some((previous_ref, _, _))) => {
                            self.report_partial(previous_ref);
                            break;
                        }
                        (result, _) => result.map_err(PdfReadError::from)?,
                    };
                    if end.pending_operands && index + 1 < self.page.contents.len() {
                        // @contract: An operator's operand span refers to its own stream.
                        self.report_partial(*content_ref);
                        break;
                    }
                    previous = Some((*content_ref, last_byte, end.open_comment));
                }
                Ok((value, _)) => {
                    drop(value);
                    self.report_partial(*content_ref);
                    break;
                }
                Err(PdfReadError::FilterUnsupported) => {
                    self.report.push(PdfReadDiagnostic::blocked(
                        PdfReadDiagnosticCode::UnsupportedFilter,
                        Some(self.page_index),
                        Some(content_ref.object_number),
                    ));
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if !self.report.partial
            && let Some(object_ref) = unverified_boundary
        {
            self.report_partial(object_ref);
        }
        drop(resource_owner);
        let invocation_bytes = self.invocation_path.capacity() * size_of::<u32>();
        drop(std::mem::take(&mut self.invocation_path));
        self.budget.release(resource_bytes + invocation_bytes);
        self.discard_saved_states(0);
        let stack_bytes = self.state_stack.capacity() * size_of::<GraphicsState>();
        drop(self.state_stack);
        self.budget.release(stack_bytes);
        let state_bytes = self.state.heap_bytes();
        // @invariant: Font allocations stay charged until the page drops all owning references.
        drop(self.state);
        self.budget.release(state_bytes);
        drop(self.fonts);
        drop(self.font_aliases);
        self.budget.release(self.font_cache_bytes);
        let mut page = PdfScenePage {
            page_index: self.page_index,
            source_object: self.page.object_ref,
            bounds: self.page.media_box,
            elements: Vec::new(),
            partial: self.report.partial,
            report: self.report,
        };
        // @invariant: Element JSON is charged before retention; this charges the rest of the page.
        self.budget.release(self.operator_diagnostic_bytes);
        self.budget.encode(&page)?;
        page.elements = self.elements;
        Ok((page, self.budget))
    }

    fn interpret_stream(
        &mut self,
        stream: &PdfStream,
        object_ref: PdfObjectRef,
        depth: usize,
        resources: Option<&PdfValue>,
        has_next_page_stream: bool,
    ) -> Result<ContentStreamEnd, ContentParseError> {
        if depth > self.limits.max_form_depth {
            self.report.push(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::FormDepthLimit,
                Some(self.page_index),
                Some(object_ref.object_number),
            ));
            return Ok(ContentStreamEnd::default());
        }
        let mut cursor = ContentCursor::new(&stream.data, self.limits.clone());
        let stack_floor = if depth == 0 {
            0
        } else {
            self.state_stack.len()
        };
        let mut operands = Vec::new();
        let mut operand_span = None::<[u32; 2]>;
        let mut payload_bytes = 0;
        let result = (|| {
            loop {
                let (token, offset, token_bytes) = match cursor
                    .next_scene_token(&mut self.remaining_content_tokens, &mut self.budget)
                {
                    Err(ContentParseError::TruncatedOperand) if has_next_page_stream => {
                        return Ok(ContentStreamEnd {
                            pending_operands: true,
                            open_comment: cursor.open_comment,
                        });
                    }
                    result => match result? {
                        Some(token) => token,
                        None => break,
                    },
                };
                payload_bytes += token_bytes;
                match token {
                    ContentToken::Word(operator) => {
                        let stream_offset = stream
                            .source_offset
                            .checked_add(offset)
                            .ok_or(PdfReadError::NumericLimit)?;
                        let stream_end = stream
                            .source_offset
                            .checked_add(cursor.position())
                            .ok_or(PdfReadError::NumericLimit)?;
                        let mut provenance_bytes =
                            operator.len() + size_of_val(self.invocation_path.as_slice());
                        let stream_offset =
                            u32::try_from(stream_offset).map_err(|_| PdfReadError::NumericLimit)?;
                        let stream_end =
                            u32::try_from(stream_end).map_err(|_| PdfReadError::NumericLimit)?;
                        self.budget.charge(provenance_bytes)?;
                        let provenance = PdfSourceProvenance {
                            object_ref,
                            stream_offset,
                            operator: Some(operator.clone()),
                            stream_end: Some(stream_end),
                            operand_span: operand_span.take(),
                            invocation_path: self.invocation_path.clone(),
                        };
                        let result = if operator == "Do" {
                            self.paint_xobject(
                                &operands,
                                provenance,
                                &mut provenance_bytes,
                                depth,
                                resources,
                            )
                        } else {
                            self.dispatch_operator(
                                &operator,
                                &operands,
                                provenance,
                                &mut provenance_bytes,
                                resources,
                                stack_floor,
                            )
                            .map_err(|error| match error {
                                PdfReadError::Syntax => ContentParseError::OperatorSyntax,
                                error => error.into(),
                            })
                        };
                        self.budget.release(provenance_bytes);
                        result?;
                        drop(operator);
                        operands.clear();
                        self.budget.release(std::mem::take(&mut payload_bytes));
                    }
                    other => {
                        let start =
                            u32::try_from(offset).map_err(|_| PdfReadError::NumericLimit)?;
                        let end = u32::try_from(cursor.position())
                            .map_err(|_| PdfReadError::NumericLimit)?;
                        operand_span.get_or_insert([start, end])[1] = end;
                        if operands.len() >= self.limits.max_array_items {
                            return Err(PdfReadError::TokenLimit.into());
                        }
                        self.budget.reserve_vec(&mut operands, 1)?;
                        operands.push(other);
                    }
                }
            }
            Ok(ContentStreamEnd {
                pending_operands: !operands.is_empty(),
                open_comment: cursor.open_comment,
            })
        })();
        let buffer_bytes = operands.capacity() * size_of::<ContentToken>();
        // @invariant: The operand buffer and payloads stay charged through dispatch and Form reads.
        drop(operands);
        self.budget.release(buffer_bytes + payload_bytes);
        result
    }

    fn dispatch_operator(
        &mut self,
        operator: &str,
        operands: &[ContentToken],
        provenance: PdfSourceProvenance,
        provenance_bytes: &mut usize,
        resources: Option<&PdfValue>,
        stack_floor: usize,
    ) -> Result<(), PdfReadError> {
        let object_ref = provenance.object_ref;
        match operator {
            "q" => self.save_state()?,
            "Q" => {
                if self.state_stack.len() == stack_floor {
                    return Err(PdfReadError::Syntax);
                }
                self.restore_state()?;
            }
            "cm" => {
                if let Some(matrix) = six_numbers(operands) {
                    let ctm = self.state.ctm.multiply(matrix)?;
                    let paint_ctm = self.state.paint_ctm.and_then(|outer| {
                        outer.multiply(PaintMatrix(std::array::from_fn(|i| {
                            operands[operands.len() - 6 + i].precise_number()
                        })))
                    });
                    self.state.path_ctm_changed |= !self.state.path.is_empty()
                        && (self.state.ctm != ctm || self.state.paint_ctm != paint_ctm);
                    self.state.ctm = ctm;
                    self.state.paint_ctm = paint_ctm;
                } else {
                    return Err(PdfReadError::Syntax);
                }
            }
            "m" => {
                let (x, y) = two_numbers(operands)?;
                let point =
                    std::array::from_fn(|i| operands[operands.len() - 2 + i].scene_decimal());
                let [px, py] = point.map(SceneDecimal::value);
                self.state.extend_path_paint_bounds([px, py, px, py]);
                self.push_path(RawPathCommand::MoveTo(x, y, point))?;
                self.state.current_point = Some((x, y));
                self.state.subpath_start = self.state.current_point;
            }
            "l" => {
                let (x, y) = two_numbers(operands)?;
                let point =
                    std::array::from_fn(|i| operands[operands.len() - 2 + i].scene_decimal());
                let [px, py] = point.map(SceneDecimal::value);
                self.state.extend_path_paint_bounds([px, py, px, py]);
                self.push_path(RawPathCommand::LineTo(x, y, point))?;
                self.state.current_point = Some((x, y));
            }
            "c" => {
                let values = six_number_values(operands)?;
                let points: [[SceneDecimal; 2]; 3] = std::array::from_fn(|i| {
                    let pair = operands.len() - 6 + 2 * i;
                    [
                        operands[pair].scene_decimal(),
                        operands[pair + 1].scene_decimal(),
                    ]
                });
                for point in points {
                    let [x, y] = point.map(SceneDecimal::value);
                    self.state.extend_path_paint_bounds([x, y, x, y]);
                }
                self.push_path(RawPathCommand::CurveTo(
                    values[0], values[1], values[2], values[3], values[4], values[5], points,
                ))?;
                self.state.current_point = Some((values[4], values[5]));
            }
            "v" => {
                let values = four_number_values(operands)?;
                let points: [[SceneDecimal; 2]; 2] = std::array::from_fn(|i| {
                    let pair = operands.len() - 4 + 2 * i;
                    [
                        operands[pair].scene_decimal(),
                        operands[pair + 1].scene_decimal(),
                    ]
                });
                for point in points {
                    let [x, y] = point.map(SceneDecimal::value);
                    self.state.extend_path_paint_bounds([x, y, x, y]);
                }
                let (x0, y0) = self.state.current_point.ok_or(PdfReadError::Syntax)?;
                self.push_path(RawPathCommand::CurveTo(
                    x0,
                    y0,
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                    [points[0], points[0], points[1]],
                ))?;
                self.state.current_point = Some((values[2], values[3]));
            }
            "y" => {
                let values = four_number_values(operands)?;
                let points: [[SceneDecimal; 2]; 2] = std::array::from_fn(|i| {
                    let pair = operands.len() - 4 + 2 * i;
                    [
                        operands[pair].scene_decimal(),
                        operands[pair + 1].scene_decimal(),
                    ]
                });
                for point in points {
                    let [x, y] = point.map(SceneDecimal::value);
                    self.state.extend_path_paint_bounds([x, y, x, y]);
                }
                self.push_path(RawPathCommand::CurveTo(
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                    values[2],
                    values[3],
                    [points[0], points[1], points[1]],
                ))?;
                self.state.current_point = Some((values[2], values[3]));
            }
            "re" => {
                let values = four_number_values(operands)?;
                let [x, y, width, height] =
                    std::array::from_fn(|i| operands[operands.len() - 4 + i].scene_decimal());
                let [px, py, right, top] = [x, y, x.sum(width), y.sum(height)];
                self.state
                    .extend_path_paint_bounds([px, py, right, top].map(SceneDecimal::value));
                let (x, y, width, height) = (values[0], values[1], values[2], values[3]);
                let x1 = x.checked_add(width)?;
                let y1 = y.checked_add(height)?;
                self.budget.reserve_vec(&mut self.state.path, 5)?;
                self.state.path.push(RawPathCommand::MoveTo(x, y, [px, py]));
                self.state
                    .path
                    .push(RawPathCommand::LineTo(x1, y, [right, py]));
                self.state
                    .path
                    .push(RawPathCommand::LineTo(x1, y1, [right, top]));
                self.state
                    .path
                    .push(RawPathCommand::LineTo(x, y1, [px, top]));
                self.state.path.push(RawPathCommand::Close);
                self.state.current_point = Some((x, y));
                self.state.subpath_start = self.state.current_point;
            }
            "h" => {
                self.push_path(RawPathCommand::Close)?;
                self.state.current_point = self.state.subpath_start;
            }
            "W" | "W*" => {
                if !self.state.path.is_empty() {
                    self.budget.charge(
                        size_of_val(self.state.path.as_slice()) + provenance.heap_bytes(),
                    )?;
                    let clip = PendingClip {
                        commands: self.state.path.clone(),
                        even_odd: operator == "W*",
                        provenance,
                        paint_bounds: self.state.path_paint_bounds,
                    };
                    if let Some(old_clip) = self.state.pending_clip.replace(clip) {
                        let bytes = old_clip.heap_bytes();
                        drop(old_clip);
                        self.budget.release(bytes);
                    }
                }
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                self.finish_path(operator, provenance)?;
            }
            "BT" => {
                self.state.in_text = true;
                self.state.text_matrix = Matrix::IDENTITY;
                self.state.line_matrix = Matrix::IDENTITY;
            }
            "ET" => self.state.in_text = false,
            "Tf" => self.set_font(operands, resources)?,
            "Tm" => {
                self.state.text_matrix = six_numbers(operands).ok_or(PdfReadError::Syntax)?;
                self.state.line_matrix = self.state.text_matrix;
            }
            "Td" | "TD" => {
                let (tx, ty) = two_numbers(operands)?;
                if operator == "TD" {
                    self.state.leading = LayoutUnit::from_raw(-ty.raw());
                }
                let translation = Matrix {
                    a: Matrix::IDENTITY.a,
                    b: LayoutUnit::from_raw(0),
                    c: LayoutUnit::from_raw(0),
                    d: Matrix::IDENTITY.d,
                    e: tx,
                    f: ty,
                };
                self.state.line_matrix = self.state.line_matrix.multiply(translation)?;
                self.state.text_matrix = self.state.line_matrix;
            }
            "T*" => self.advance_line()?,
            "TL" => self.state.leading = one_number(operands)?,
            "Tc" => self.state.char_spacing = one_number(operands)?,
            "Tw" => self.state.word_spacing = one_number(operands)?,
            "Tz" => {
                let value = one_number(operands)?;
                self.state.horizontal_scale = scale_percent(value)?;
            }
            "Ts" => self.state.rise = one_number(operands)?,
            "Tj" | "'" | "\"" => {
                if operator == "'" {
                    self.advance_line()?;
                }
                if let Some(ContentToken::String(bytes)) = operands.last() {
                    self.show_text(bytes, &provenance)?;
                }
            }
            "TJ" => {
                if let Some(ContentToken::Array(values)) = operands.last() {
                    for value in values {
                        match value {
                            ContentToken::String(bytes) => self.show_text(bytes, &provenance)?,
                            ContentToken::Integer(value) => {
                                let adjustment = LayoutUnit::from_raw(
                                    value.checked_mul(-1).ok_or(PdfReadError::NumericLimit)?,
                                );
                                let adjustment =
                                    scale_font_width(adjustment, self.state.font_size)?;
                                self.advance_text(adjustment)?;
                            }
                            _ => return Err(PdfReadError::Syntax),
                        }
                    }
                }
            }
            "w" => self.state.line_width = precise_operand(operands),
            "M" => self.state.miter_limit = precise_operand(operands),
            "d" => {
                self.state.solid_stroke = matches!(operands, [ContentToken::Array(pattern), phase]
                    if pattern.is_empty() && phase.number().is_some());
            }
            "gs" => {
                self.state.solid_stroke = false;
                self.state.line_width = None;
                self.state.miter_limit = None;
            }
            known if is_known_noop(known) => {}
            unsupported => {
                let diagnostic = if matches!(unsupported, "BI" | "ID" | "EI") {
                    PdfReadDiagnostic::blocked(
                        PdfReadDiagnosticCode::UnsupportedOperator,
                        Some(self.page_index),
                        Some(object_ref.object_number),
                    )
                } else {
                    PdfReadDiagnostic::warning(
                        PdfReadDiagnosticCode::UnsupportedOperator,
                        Some(self.page_index),
                        Some(object_ref.object_number),
                    )
                };
                let diagnostic = PdfReadDiagnostic {
                    operator: provenance.operator,
                    ..diagnostic
                };
                if !self.report.diagnostics.contains(&diagnostic) {
                    let bytes = diagnostic.operator.as_ref().map_or(0, String::capacity);
                    *provenance_bytes -= bytes;
                    self.operator_diagnostic_bytes += bytes;
                }
                self.report.push(diagnostic);
            }
        }
        Ok(())
    }

    fn push_path(&mut self, command: RawPathCommand) -> Result<(), PdfReadError> {
        self.budget.reserve_vec(&mut self.state.path, 1)?;
        self.state.path.push(command);
        Ok(())
    }

    fn reserve_saved_state(&mut self) -> Result<(), PdfReadError> {
        // @invariant: The stack also holds callers' saved states during Form interpretation.
        if self.state_stack.len() >= self.limits.max_nesting {
            return Err(PdfReadError::TokenLimit);
        }
        self.budget.reserve_vec(&mut self.state_stack, 1)
    }

    fn save_state(&mut self) -> Result<(), PdfReadError> {
        self.reserve_saved_state()?;
        self.budget.charge(self.state.clone_heap_bytes())?;
        self.state_stack.push(self.state.clone());
        Ok(())
    }

    fn save_form_caller(&mut self) -> Result<(), PdfReadError> {
        self.reserve_saved_state()?;
        // @invariant: The caller keeps ownership of its charged path and pending clip.
        let path = std::mem::take(&mut self.state.path);
        let pending_clip = self.state.pending_clip.take();
        self.state_stack.push(GraphicsState {
            path,
            pending_clip,
            ..self.state.clone()
        });
        Ok(())
    }

    fn restore_state(&mut self) -> Result<(), PdfReadError> {
        let state = self.state_stack.pop().ok_or(PdfReadError::Syntax)?;
        let discarded = std::mem::replace(&mut self.state, state);
        let bytes = discarded.heap_bytes();
        // @invariant: Restoring a state transfers its allocation charge to the active state.
        drop(discarded);
        self.budget.release(bytes);
        Ok(())
    }

    fn discard_saved_states(&mut self, keep: usize) {
        for state in self.state_stack.drain(keep..) {
            let bytes = state.heap_bytes();
            drop(state);
            self.budget.release(bytes);
        }
    }

    fn advance_line(&mut self) -> Result<(), PdfReadError> {
        let translation = Matrix {
            a: Matrix::IDENTITY.a,
            b: LayoutUnit::from_raw(0),
            c: LayoutUnit::from_raw(0),
            d: Matrix::IDENTITY.d,
            e: LayoutUnit::from_raw(0),
            f: LayoutUnit::from_raw(
                self.state
                    .leading
                    .raw()
                    .checked_neg()
                    .ok_or(PdfReadError::NumericLimit)?,
            ),
        };
        self.state.line_matrix = self.state.line_matrix.multiply(translation)?;
        self.state.text_matrix = self.state.line_matrix;
        Ok(())
    }

    fn finish_path(
        &mut self,
        operator: &str,
        provenance: PdfSourceProvenance,
    ) -> Result<(), PdfReadError> {
        let pending_clip = self.state.pending_clip.take();
        let paint_bounds = self.state.path_paint_bounds.take();
        let path_ctm_changed = std::mem::take(&mut self.state.path_ctm_changed);
        self.state.current_point = None;
        self.state.subpath_start = None;

        if path_ctm_changed {
            if pending_clip.is_some() {
                self.state.clip.rectangular = false;
            }
            if pending_clip.is_some() || (!self.state.path.is_empty() && operator != "n") {
                self.report_partial(provenance.object_ref);
            }
            self.state.path.clear();
            if let Some(clip) = pending_clip {
                let bytes = clip.heap_bytes();
                drop(clip);
                self.budget.release(bytes);
            }
            return Ok(());
        }

        if !self.state.path.is_empty() && operator != "n" {
            let close = matches!(operator, "s" | "b" | "b*")
                && !matches!(self.state.path.last(), Some(RawPathCommand::Close));
            let commands = transform_path(&self.state.path, self.state.ctm, close, self.budget)?;
            let rect = path_bounds(&commands)?;
            let stroke = matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*");
            let fill = !matches!(operator, "S" | "s");
            let visible = if stroke {
                if fill
                    && self.state.clip.intersect(rect, true)?.is_empty()
                    && self.paint_may_be_visible(paint_bounds)
                {
                    self.report_partial(provenance.object_ref);
                }
                self.stroke_visible(rect, paint_bounds, &provenance)?
            } else {
                self.bounded_paint_visible(rect, paint_bounds, &provenance)?
            };
            let lost_fill = fill
                && (visible || (!stroke && paint_bounds.is_some_and(PaintBounds::is_empty)))
                && lost_visible_fill_contour(
                    &self.state.path,
                    &commands,
                    self.state.paint_ctm,
                    self.state.paint_clip,
                );
            // @contract: Only provenance records the fill rule for painted paths.
            if lost_fill
                || (visible
                    && (matches!(operator, "f*" | "B*" | "b*")
                        || (stroke && contains_collapsed_stroke(&commands, &self.state.path))))
            {
                self.report_partial(provenance.object_ref);
            }
            if visible {
                let heap_bytes =
                    commands.capacity() * size_of::<PdfPathCommand>() + provenance.heap_bytes();
                self.push_element_with_workspace(
                    PdfSceneElement::Path {
                        value: PdfPathElement {
                            commands,
                            rect,
                            stroke,
                            fill,
                            provenance,
                        },
                    },
                    heap_bytes,
                )?;
            }
        }
        self.state.path.clear();
        if let Some(PendingClip {
            commands,
            even_odd,
            provenance: clip_provenance,
            paint_bounds,
        }) = pending_clip
        {
            if let Some(bounds) = paint_bounds {
                self.state.paint_clip = self.state.paint_clip.intersect(bounds);
            }
            let rectangular = paint_bounds.is_some()
                && self
                    .state
                    .paint_ctm
                    .is_some_and(|matrix| is_precise_rectangular_path(&commands, matrix));
            let transformed = transform_path(&commands, self.state.ctm, false, self.budget)?;
            let raw_bytes = commands.capacity() * size_of::<RawPathCommand>();
            drop(commands);
            self.budget.release(raw_bytes);
            let rect = path_bounds(&transformed)?;
            self.state.clip = self
                .state
                .clip
                .intersect(rect, rectangular && is_rectangular_path(&transformed))?;
            let provenance_bytes = clip_provenance.heap_bytes();
            let heap_bytes =
                transformed.capacity() * size_of::<PdfPathCommand>() + provenance_bytes;
            self.budget.release(provenance_bytes);
            self.push_element_with_workspace(
                PdfSceneElement::Clip {
                    value: PdfClipElement {
                        rect,
                        even_odd,
                        provenance: clip_provenance,
                        commands: transformed,
                    },
                },
                heap_bytes,
            )?;
        }
        Ok(())
    }

    fn report_partial(&mut self, object_ref: PdfObjectRef) {
        self.report.partial = true;
        self.report.push(PdfReadDiagnostic::warning(
            PdfReadDiagnosticCode::PartialPage,
            Some(self.page_index),
            Some(object_ref.object_number),
        ));
    }

    fn stroke_visible(
        &mut self,
        rect: LayoutRect,
        paint_bounds: Option<PaintBounds>,
        provenance: &PdfSourceProvenance,
    ) -> Result<bool, PdfReadError> {
        let clip = self.state.clip;
        let stroke_bounds = paint_bounds.and_then(|bounds| self.stroke_bounds(bounds));
        if !self.paint_may_be_visible(stroke_bounds) {
            return Ok(false);
        }
        if clip.is_empty() {
            self.report_partial(provenance.object_ref);
            return Ok(false);
        }
        // @contract: Stroke paint can extend beyond the scene's centerline bounds.
        if !clip.rectangular
            || !clip.contains(rect)?
            || !self.state.solid_stroke
            || !stroke_bounds.is_some_and(|bounds| self.state.paint_clip.contains(bounds))
        {
            self.report_partial(provenance.object_ref);
        }
        Ok(true)
    }

    fn stroke_bounds(&self, bounds: PaintBounds) -> Option<PaintBounds> {
        let width = self.state.line_width?;
        let miter = self.state.miter_limit?;
        if width <= 0. || miter < 1. {
            return None;
        }
        let PaintMatrix([a, b, c, d, ..]) = self.state.paint_ctm?;
        let scale = (a.abs() + c.abs()).max(b.abs() + d.abs());
        // @invariant: The margin covers miter joins and projecting square caps.
        bounds.expanded(width * 0.5 * miter.max(2.) * scale)
    }

    fn paint_may_be_visible(&self, paint_bounds: Option<PaintBounds>) -> bool {
        let clip = self.state.paint_clip;
        !clip.is_empty() && paint_bounds.is_none_or(|bounds| !clip.intersect(bounds).is_empty())
    }

    fn bounded_paint_visible(
        &mut self,
        rect: LayoutRect,
        paint_bounds: Option<PaintBounds>,
        provenance: &PdfSourceProvenance,
    ) -> Result<bool, PdfReadError> {
        if !self.paint_may_be_visible(paint_bounds) {
            return Ok(false);
        }
        let intersection = self.state.clip.intersect(rect, true)?;
        if intersection.is_empty() {
            self.report_partial(provenance.object_ref);
            return Ok(false);
        }
        if !intersection.rectangular
            || intersection.bounds != rect
            || !paint_bounds.is_some_and(|bounds| self.state.paint_clip.contains(bounds))
        {
            self.report_partial(provenance.object_ref);
        }
        Ok(true)
    }

    fn resource_reference(
        &self,
        resources: Option<&PdfValue>,
        category: &str,
        name: &str,
    ) -> Result<PdfObjectRef, PdfReadError> {
        let value = resources
            .and_then(PdfValue::as_dictionary)
            .and_then(|dictionary| dictionary.get(category))
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let resolved;
        let value = if let PdfValue::Reference(reference) = value {
            resolved = self
                .reader
                .scene_object(*reference, 0, self.budget.remaining_scene_bytes)?
                .0;
            &resolved
        } else {
            value
        };
        value
            .as_dictionary()
            .and_then(|entries| entries.get(name))
            .and_then(PdfValue::as_reference)
            .ok_or(PdfReadError::ReferenceInvalid)
    }

    fn set_font(
        &mut self,
        operands: &[ContentToken],
        resources: Option<&PdfValue>,
    ) -> Result<(), PdfReadError> {
        let Some(ContentToken::Name(name)) = operands.first() else {
            return Err(PdfReadError::Syntax);
        };
        let size = operands
            .get(1)
            .and_then(ContentToken::number)
            .ok_or(PdfReadError::Syntax)?;
        let font_ref = self.resource_reference(resources, "Font", name)?;
        let mut budget = self.budget;
        let cache_font = !self.fonts.contains_key(&font_ref);
        let map = if let Some(font) = self.fonts.get(&font_ref) {
            Rc::clone(font)
        } else {
            let font = self.load_font(font_ref, &mut budget)?;
            budget.charge(
                size_of::<FontMap>()
                    + 2 * size_of::<usize>()
                    + scene_tree_entry_bytes::<(PdfObjectRef, Rc<FontMap>)>(),
            )?;
            Rc::new(font)
        };
        let mut fallback_name = None;
        if map.base_name.is_none()
            && let Some(label) = scene_name_label(name)
        {
            let alias = if let Some(alias) = self.font_aliases.get(label) {
                Rc::clone(alias)
            } else {
                budget.charge(2 * size_of::<usize>() + scene_tree_entry_bytes::<Rc<str>>())?;
                budget.charge(label.len().next_multiple_of(align_of::<usize>()))?;
                let alias: Rc<str> = Rc::from(label);
                self.font_aliases.insert(Rc::clone(&alias));
                alias
            };
            fallback_name = Some(alias);
        }
        if cache_font {
            self.fonts.insert(font_ref, Rc::clone(&map));
        }
        self.font_cache_bytes += self.budget.remaining_scene_bytes - budget.remaining_scene_bytes;
        self.budget = budget;
        self.state.font = Some(SelectedFont { map, fallback_name });
        self.state.font_size = size;
        Ok(())
    }

    fn load_font(
        &self,
        font_ref: PdfObjectRef,
        budget: &mut SceneBudget,
    ) -> Result<FontMap, PdfReadError> {
        let (font, font_bytes) =
            self.reader
                .scene_object(font_ref, 0, budget.remaining_scene_bytes)?;
        budget.charge(font_bytes)?;
        let dictionary = font.as_dictionary().ok_or(PdfReadError::ReferenceInvalid)?;
        let base_name = dictionary.get("BaseFont").and_then(PdfValue::as_name);
        budget.charge(base_name.map_or(0, str::len))?;
        let base_name = base_name.map(str::to_owned);
        let to_unicode_ref = dictionary
            .get("ToUnicode")
            .map(|value| value.as_reference().ok_or(PdfReadError::ReferenceInvalid))
            .transpose()?;
        let simple_encoding = dictionary
            .get("Subtype")
            .and_then(PdfValue::as_name)
            .is_none_or(|value| value != "Type0");
        let default_width = dictionary
            .get("MissingWidth")
            .and_then(font_width)
            .unwrap_or(LayoutUnit::from_raw(500));
        let mut widths = BTreeMap::new();
        if let Some(values) = dictionary.get("Widths").and_then(PdfValue::as_array) {
            let first = dictionary
                .get("FirstChar")
                .and_then(PdfValue::as_integer)
                .unwrap_or(0);
            let codes = values.iter().enumerate().filter_map(|(index, value)| {
                let code = first.checked_add(i64::try_from(index).ok()?)?;
                Some((u8::try_from(code).ok()?, value))
            });
            for (code, value) in codes {
                budget.charge(scene_tree_entry_bytes::<(u8, LayoutUnit)>())?;
                widths.insert(code, font_width(value).unwrap_or(default_width));
            }
        }
        drop(font);
        budget.release(font_bytes);
        let mut to_unicode = BTreeMap::new();
        if let Some(reference) = to_unicode_ref {
            let (object, stream_bytes) =
                self.reader
                    .scene_object(reference, 0, budget.remaining_scene_bytes)?;
            budget.charge(stream_bytes)?;
            if let PdfValue::Stream(stream) = &object {
                to_unicode = parse_to_unicode(&stream.data, self.limits.max_array_items, budget)?;
            }
            drop(object);
            budget.release(stream_bytes);
        }
        Ok(FontMap {
            base_name,
            to_unicode,
            simple_encoding,
            widths,
            default_width,
        })
    }

    fn show_text(
        &mut self,
        bytes: &[u8],
        provenance: &PdfSourceProvenance,
    ) -> Result<(), PdfReadError> {
        if !self.state.in_text {
            return Err(PdfReadError::Syntax);
        }
        let selected_font = self
            .state
            .font
            .clone()
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let font = &selected_font.map;
        let mut workspace = self.budget;
        let provenance_bytes = provenance.clone_heap_bytes();
        let mut glyph_count = 0;
        let mut text_bytes = 0;
        // @invariant: Glyph storage, mapped strings, run text, and glyph provenance coexist.
        for (_, unicode) in font.glyphs(bytes) {
            if glyph_count >= self.limits.max_scene_elements {
                return Err(PdfReadError::TokenLimit);
            }
            let unicode_bytes = unicode.map_or(0, str::len);
            workspace.charge(size_of::<PdfGlyph>() + provenance_bytes + 2 * unicode_bytes)?;
            glyph_count += 1;
            text_bytes += unicode_bytes;
        }
        let mut glyphs = Vec::with_capacity(glyph_count);
        let mut text = String::with_capacity(text_bytes);
        let mut mapping = if font.to_unicode.is_empty() {
            if font.simple_encoding {
                PdfTextMapping::SimpleEncoding
            } else {
                PdfTextMapping::Missing
            }
        } else {
            PdfTextMapping::ToUnicode
        };
        for (code, unicode) in font.glyphs(bytes) {
            if let Some(unicode) = unicode {
                text.push_str(unicode);
            } else {
                mapping = PdfTextMapping::Missing;
                self.report.push(
                    PdfReadDiagnostic::warning(
                        PdfReadDiagnosticCode::MissingUnicodeMapping,
                        Some(self.page_index),
                        Some(provenance.object_ref.object_number),
                    )
                    .with_operator(provenance.operator.as_deref().unwrap_or("Tj")),
                );
            }
            let width_units = font
                .widths
                .get(&code)
                .copied()
                .unwrap_or(font.default_width);
            let advance = scale_font_width(width_units, self.state.font_size)?;
            let advance = advance
                .checked_add(self.state.char_spacing)
                .and_then(|value| {
                    if code == b' ' {
                        value.checked_add(self.state.word_spacing)
                    } else {
                        Ok(value)
                    }
                })?;
            let advance = mul(advance, self.state.horizontal_scale)?;
            let transform = self.state.ctm.multiply(self.state.text_matrix)?;
            let zero = LayoutUnit::from_raw(0);
            let rect = if self.invocation_path.len() > 1 {
                transformed_rect(
                    transform,
                    LayoutRect {
                        x: zero,
                        y: self.state.rise.checked_sub(self.state.font_size)?,
                        width: advance,
                        height: self.state.font_size,
                    },
                )?
            } else {
                let (x, y) = transform.point(zero, self.state.rise)?;
                LayoutRect {
                    x,
                    y: y.checked_sub(self.state.font_size)?,
                    width: advance,
                    height: self.state.font_size,
                }
            };
            glyphs.push(PdfGlyph {
                code: u32::from(code),
                unicode: unicode.map(str::to_owned),
                rect,
                advance,
                provenance: provenance.clone(),
            });
            self.advance_text(advance)?;
        }
        if !glyphs.is_empty() {
            if self.state.clip.is_empty() {
                if !self.state.paint_clip.is_empty() {
                    self.report_partial(provenance.object_ref);
                }
                return Ok(());
            }
            let rect = union_rect(glyphs.iter().map(|glyph| glyph.rect))?;
            // @contract: Advance rectangles do not bound painted glyphs.
            self.report_partial(provenance.object_ref);
            if !self.emit_elements {
                return Ok(());
            }
            let font_name = font
                .base_name
                .as_deref()
                .or(selected_font.fallback_name.as_deref());
            workspace.charge(provenance_bytes + font_name.map_or(0, str::len))?;
            let retained_bytes =
                self.budget.remaining_scene_bytes - workspace.remaining_scene_bytes;
            self.push_element_with_workspace(
                PdfSceneElement::Text {
                    value: PdfTextElement {
                        text,
                        rect,
                        font_name: font_name.map(str::to_owned),
                        mapping,
                        glyphs,
                        provenance: provenance.clone(),
                    },
                },
                retained_bytes,
            )?;
        }
        Ok(())
    }

    fn advance_text(&mut self, advance: LayoutUnit) -> Result<(), PdfReadError> {
        self.state.text_matrix.e = self.state.text_matrix.e.checked_add(advance)?;
        Ok(())
    }

    fn paint_xobject(
        &mut self,
        operands: &[ContentToken],
        provenance: PdfSourceProvenance,
        provenance_bytes: &mut usize,
        depth: usize,
        resources: Option<&PdfValue>,
    ) -> Result<(), ContentParseError> {
        let Some(ContentToken::Name(name)) = operands.last() else {
            return Err(ContentParseError::OperatorSyntax);
        };
        let xobject_ref = self.resource_reference(resources, "XObject", name)?;
        self.with_scene_object(xobject_ref, |builder, xobject| {
            let PdfValue::Stream(stream) = xobject else {
                return Err(PdfReadError::ReferenceInvalid);
            };
            match stream.dict.get("Subtype").and_then(PdfValue::as_name) {
                Some("Image") => builder.paint_image(stream, xobject_ref, provenance),
                Some("Form") => {
                    if depth + 1 > builder.limits.max_form_depth {
                        builder.report.push(PdfReadDiagnostic::blocked(
                            PdfReadDiagnosticCode::FormDepthLimit,
                            Some(builder.page_index),
                            Some(xobject_ref.object_number),
                        ));
                        return Ok(());
                    }
                    let paint = |builder: &mut Self, local_resources: Option<&PdfValue>| {
                        let effective_resources = local_resources
                            .filter(|value| !matches!(value, PdfValue::Null))
                            .or(resources);
                        builder.paint_form(
                            stream,
                            xobject_ref,
                            provenance,
                            provenance_bytes,
                            depth,
                            effective_resources,
                        )
                    };
                    match stream.dict.get("Resources") {
                        Some(PdfValue::Reference(reference)) => builder
                            .with_scene_object(*reference, |builder, value| {
                                paint(builder, Some(value))
                            }),
                        value => paint(builder, value),
                    }
                }
                _ => {
                    builder.report.push(
                        PdfReadDiagnostic::warning(
                            PdfReadDiagnosticCode::UnsupportedOperator,
                            Some(builder.page_index),
                            Some(provenance.object_ref.object_number),
                        )
                        .with_operator("Do"),
                    );
                    Ok(())
                }
            }
        })
        .map_err(ContentParseError::from)
    }

    fn with_scene_object(
        &mut self,
        reference: PdfObjectRef,
        use_object: impl FnOnce(&mut Self, &PdfValue) -> Result<(), PdfReadError>,
    ) -> Result<(), PdfReadError> {
        let (value, bytes) =
            self.reader
                .scene_object(reference, 0, self.budget.remaining_scene_bytes)?;
        // @invariant: The owner stays charged across nested interpretation and error returns.
        self.budget.charge(bytes)?;
        let result = use_object(self, &value);
        drop(value);
        self.budget.release(bytes);
        result
    }

    fn paint_form(
        &mut self,
        stream: &PdfStream,
        xobject_ref: PdfObjectRef,
        provenance: PdfSourceProvenance,
        provenance_bytes: &mut usize,
        depth: usize,
        effective_resources: Option<&PdfValue>,
    ) -> Result<(), PdfReadError> {
        let dictionary = &stream.dict;
        let mut form_paint_ctm = self.state.paint_ctm;
        let form_ctm = match dictionary.get("Matrix") {
            None | Some(PdfValue::Null) => self.state.ctm,
            Some(value) => {
                let resolved;
                let value = if let PdfValue::Reference(reference) = value {
                    resolved = self
                        .reader
                        .scene_object(*reference, 0, self.budget.remaining_scene_bytes)?
                        .0;
                    &resolved
                } else {
                    value
                };
                match value {
                    PdfValue::Null => self.state.ctm,
                    value => {
                        let Ok(matrix) = Matrix::from_pdf_array(value) else {
                            self.report_partial(xobject_ref);
                            return Ok(());
                        };
                        form_paint_ctm = form_paint_ctm.and_then(|outer| {
                            outer.multiply(PaintMatrix(PaintMatrix::pdf_numbers(value)?))
                        });
                        self.state.ctm.multiply(matrix)?
                    }
                }
            }
        };
        let (precise_bbox, bbox) = {
            let resolved;
            let value = match dictionary.get("BBox") {
                Some(PdfValue::Reference(reference)) => {
                    resolved = self
                        .reader
                        .scene_object(*reference, depth + 1, self.budget.remaining_scene_bytes)?
                        .0;
                    Some(&resolved)
                }
                value => value,
            };
            let precise = value.and_then(|value| {
                let [x, y, right, top] = PaintMatrix::pdf_numbers(value)?;
                (x < right && y < top).then_some([x, y, right, top])
            });
            let bbox = value
                .and_then(|value| parse_bbox(value).ok())
                .filter(|bbox| {
                    precise.is_some() || (bbox.width.raw() > 0 && bbox.height.raw() > 0)
                });
            (precise, bbox)
        };
        let Some(bbox) = bbox else {
            self.report_partial(xobject_ref);
            return Ok(());
        };
        let form_paint_bounds = form_paint_ctm.and_then(|matrix| matrix.bounds(precise_bbox?));
        let form_paint_clip = form_paint_bounds.map_or(self.state.paint_clip, |bounds| {
            self.state.paint_clip.intersect(bounds)
        });
        let form_bounds = transformed_rect(form_ctm, bbox)?;
        let zero = LayoutUnit::from_raw(0);
        let rectangular = ((form_ctm.b == zero && form_ctm.c == zero)
            || (form_ctm.a == zero && form_ctm.d == zero))
            && form_paint_bounds.is_some()
            && form_paint_ctm.is_some_and(PaintMatrix::is_axis_aligned);
        let clip = self.state.clip.intersect(form_bounds, rectangular)?;
        let stream_offset = provenance.stream_offset;
        let subtype = "FormXObject";
        self.budget.remaining_after(subtype.len())?;
        let retained = provenance.heap_bytes() + subtype.len();
        let element = PdfSceneElement::Annotation {
            value: PdfAnnotationElement {
                subtype: Some(subtype.to_owned()),
                kind: PdfAnnotationKind::Supported,
                rect: form_bounds,
                provenance: PdfSourceProvenance {
                    object_ref: xobject_ref,
                    ..provenance
                },
            },
        };
        self.budget.release(std::mem::take(provenance_bytes));
        self.push_element_with_workspace(element, retained)?;
        if clip.is_empty() && form_paint_clip.is_empty() {
            return Ok(());
        }
        self.budget.reserve_vec(&mut self.invocation_path, 1)?;
        let caller_state_index = self.state_stack.len();
        self.save_form_caller()?;
        let caller_emit_elements = self.emit_elements;
        self.emit_elements &= !clip.is_empty();
        self.state.ctm = form_ctm;
        self.state.paint_ctm = form_paint_ctm;
        self.state.paint_clip = form_paint_clip;
        self.state.path_paint_bounds = None;
        self.state.clip = clip;
        self.state.current_point = None;
        self.state.subpath_start = None;
        self.state.path_ctm_changed = false;
        self.invocation_path.push(stream_offset);
        let result =
            self.interpret_stream(stream, xobject_ref, depth + 1, effective_resources, false);
        self.invocation_path.pop();
        self.emit_elements = caller_emit_elements;
        if self.state_stack.len() > caller_state_index + 1 {
            self.report_partial(xobject_ref);
        }
        self.discard_saved_states(caller_state_index + 1);
        self.restore_state()?;
        result.map(|_| ()).map_err(PdfReadError::from)
    }

    fn paint_image(
        &mut self,
        stream: &PdfStream,
        object_ref: PdfObjectRef,
        provenance: PdfSourceProvenance,
    ) -> Result<(), PdfReadError> {
        let width = stream
            .dict
            .get("Width")
            .and_then(PdfValue::as_integer)
            .ok_or(PdfReadError::ImageLimit)?;
        let height = stream
            .dict
            .get("Height")
            .and_then(PdfValue::as_integer)
            .ok_or(PdfReadError::ImageLimit)?;
        let width = u32::try_from(width).map_err(|_| PdfReadError::ImageLimit)?;
        let height = u32::try_from(height).map_err(|_| PdfReadError::ImageLimit)?;
        if u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or(PdfReadError::ImageLimit)?
            > self.limits.max_image_pixels
        {
            return Err(PdfReadError::ImageLimit);
        }
        let filter = stream
            .dict
            .get("Filter")
            .and_then(single_filter)
            .unwrap_or("Identity")
            .to_owned();
        let media_type = if filter == "DCTDecode" || filter == "DCT" {
            "image/jpeg"
        } else {
            "application/octet-stream"
        };
        let zero = LayoutUnit::from_raw(0);
        let one = LayoutUnit::from_raw(LayoutUnit::UNITS_PER_POINT);
        let rect = transformed_rect(
            self.state.ctm,
            LayoutRect {
                x: zero,
                y: zero,
                width: one,
                height: one,
            },
        )?;
        let paint_bounds = self
            .state
            .paint_ctm
            .and_then(|matrix| matrix.bounds([0., 0., 1., 1.]));
        if !self.bounded_paint_visible(rect, paint_bounds, &provenance)? {
            return Ok(());
        }
        let Matrix { a, b, c, d, .. } = self.state.ctm;
        // @contract: An image rectangle carries only positive axis-aligned placement.
        if a <= zero
            || d <= zero
            || b != zero
            || c != zero
            || !self
                .state
                .paint_ctm
                .is_some_and(PaintMatrix::supports_image_rect)
        {
            self.report_partial(provenance.object_ref);
            return Ok(());
        }
        if !self.emit_elements {
            return Ok(());
        }
        let Some(data_hex) = self.budget.image_hex(&stream.data)? else {
            self.report.push(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::ImageLimit,
                Some(self.page_index),
                Some(object_ref.object_number),
            ));
            return Ok(());
        };
        let heap_bytes =
            data_hex.capacity() + filter.capacity() + media_type.len() + provenance.heap_bytes();
        self.push_element_with_workspace(
            PdfSceneElement::Image {
                value: PdfImageElement {
                    width,
                    height,
                    media_type: media_type.to_owned(),
                    filter,
                    data_hex,
                    rect,
                    provenance: PdfSourceProvenance {
                        object_ref,
                        ..provenance
                    },
                },
            },
            heap_bytes,
        )
    }

    fn inspect_annotations(&mut self) -> Result<(), PdfReadError> {
        for annotation_ref in &self.page.annotations {
            let (value, annotation_bytes) =
                self.reader
                    .scene_object(*annotation_ref, 0, self.budget.remaining_scene_bytes)?;
            self.budget.charge(annotation_bytes)?;
            let dictionary = value
                .as_dictionary()
                .ok_or(PdfReadError::ReferenceInvalid)?;
            let subtype = dictionary.get("Subtype").and_then(PdfValue::as_name);
            let rect = dictionary
                .get("Rect")
                .and_then(|value| parse_bbox(value).ok())
                .unwrap_or(LayoutRect {
                    x: LayoutUnit::from_raw(0),
                    y: LayoutUnit::from_raw(0),
                    width: LayoutUnit::from_raw(0),
                    height: LayoutUnit::from_raw(0),
                });
            let provenance = PdfSourceProvenance {
                object_ref: *annotation_ref,
                stream_offset: u32::try_from(
                    self.reader.object_offset(*annotation_ref).unwrap_or(0),
                )
                .map_err(|_| PdfReadError::NumericLimit)?,
                operator: None,
                stream_end: None,
                operand_span: None,
                invocation_path: Vec::new(),
            };
            let (element, heap_bytes) = if subtype == Some("Link") {
                let action = dictionary.get("A").and_then(PdfValue::as_dictionary);
                let goto = action
                    .filter(|action| action.get("S").and_then(PdfValue::as_name) == Some("GoTo"));
                if action.is_some() && goto.is_none() {
                    self.report.push(PdfReadDiagnostic::blocked(
                        PdfReadDiagnosticCode::ActiveContent,
                        Some(self.page_index),
                        Some(annotation_ref.object_number),
                    ));
                }
                let target = dictionary
                    .get("Dest")
                    .or_else(|| goto.and_then(|action| action.get("D")));
                let destination = if target.is_some() || goto.is_some() {
                    self.read_link_destination(*annotation_ref, target)?
                } else {
                    None
                };
                (
                    PdfSceneElement::Link {
                        value: PdfLinkElement {
                            destination_page: destination,
                            rect,
                            internal: destination.is_some(),
                            provenance,
                        },
                    },
                    0,
                )
            } else if subtype == Some("Widget") {
                if self.field_root_counts.is_none() {
                    let workspace = Cell::new(self.budget.remaining_scene_bytes);
                    let counts = read_field_root_counts(self.reader, &workspace)?;
                    self.budget.remaining_scene_bytes = workspace.get();
                    *self.field_root_counts = Some(counts);
                }
                let widget = self.read_widget(&value, dictionary, rect, provenance)?;
                let heap_bytes = [&widget.field_type, &widget.name, &widget.value]
                    .into_iter()
                    .flatten()
                    .map(String::capacity)
                    .sum();
                (PdfSceneElement::Form { value: widget }, heap_bytes)
            } else {
                self.report.push(PdfReadDiagnostic::warning(
                    PdfReadDiagnosticCode::UnsupportedAnnotation,
                    Some(self.page_index),
                    Some(annotation_ref.object_number),
                ));
                let mut remaining = self.budget.remaining_scene_bytes;
                let subtype = copy_scene_string(subtype, &mut remaining)?;
                let heap_bytes = subtype.as_ref().map_or(0, String::capacity);
                (
                    PdfSceneElement::Annotation {
                        value: PdfAnnotationElement {
                            subtype,
                            kind: PdfAnnotationKind::Unsupported,
                            rect,
                            provenance,
                        },
                    },
                    heap_bytes,
                )
            };
            for key in ["AA", "JS", "JavaScript", "Launch", "GoToR", "URI", "FS"] {
                if dictionary.contains_key(key) {
                    self.report.push(PdfReadDiagnostic::blocked(
                        if key == "FS" {
                            PdfReadDiagnosticCode::ExternalResource
                        } else {
                            PdfReadDiagnosticCode::ActiveContent
                        },
                        Some(self.page_index),
                        Some(annotation_ref.object_number),
                    ));
                }
            }
            drop(value);
            self.budget.release(annotation_bytes);
            self.push_element_with_workspace(element, heap_bytes)?;
        }
        Ok(())
    }

    fn read_widget(
        &mut self,
        annotation: &PdfValue,
        dictionary: &BTreeMap<String, PdfValue>,
        rect: LayoutRect,
        provenance: PdfSourceProvenance,
    ) -> Result<PdfFormElement, PdfReadError> {
        let widget = provenance.object_ref;
        let workspace = Cell::new(self.budget.remaining_scene_bytes);
        let owner = self.widget_field_owner(widget, dictionary, &workspace)?;
        let field = owner
            .as_deref()
            .and_then(PdfValue::as_dictionary)
            .unwrap_or(dictionary);
        let limit = self.limits.max_string_bytes;
        let field_root_counts = self.field_root_counts.as_ref();
        let name = map_widget_label_with_workspace(
            widget,
            dictionary,
            self.limits.max_nesting,
            &mut self.widget_work,
            &workspace,
            |reference| {
                if reference == widget {
                    Ok(Some(AnnotationValue::borrowed(annotation)))
                } else {
                    self.reader.scene_label_object(reference, 0, &workspace)
                }
            },
            |field, _| {
                Ok(field_root_counts
                    .and_then(|counts| counts.get(&field))
                    .copied()
                    .unwrap_or(0))
            },
            |bytes| {
                let mut remaining = workspace.get();
                let name = decode_pdf_text_string(bytes, limit, &mut remaining)?
                    .filter(|name| !name.is_empty() && !name.chars().any(char::is_control));
                if name.is_some() {
                    workspace.set(remaining);
                }
                Ok(name)
            },
        )?
        .flatten();
        let mut remaining = workspace.get();
        let field_type = field.get("FT").and_then(PdfValue::as_name);
        let saved = field.get("V");
        let value = if field_type == Some("Btn")
            && field
                .get("Ff")
                .is_none_or(|flags| matches!(flags.as_integer(), Some(0..=7)))
        {
            copy_scene_string(saved.and_then(PdfValue::as_name), &mut remaining)?
        } else if let Some(bytes) = saved.and_then(PdfValue::as_bytes) {
            decode_pdf_text_string(bytes, limit, &mut remaining)?
        } else {
            None
        };
        Ok(PdfFormElement {
            field_type: copy_scene_string(field_type, &mut remaining)?,
            name,
            value,
            rect,
            provenance,
        })
    }

    fn read_link_destination(
        &mut self,
        annotation_ref: PdfObjectRef,
        target: Option<&PdfValue>,
    ) -> Result<Option<u32>, PdfReadError> {
        let workspace = Cell::new(self.budget.remaining_scene_bytes);
        let resolved = match target {
            Some(value) => {
                self.reader
                    .link_destination_value(value, &mut self.link_work, &workspace)
            }
            None => Ok(None),
        };
        let code = match resolved {
            Ok(value) => {
                if let Some(page_index) = value
                    .as_deref()
                    .and_then(PdfValue::as_array)
                    .and_then(explicit_destination_target)
                    .and_then(|reference| {
                        let index = self.page_refs.partition_point(|(key, _)| *key <= reference);
                        self.page_refs[..index]
                            .last()
                            .filter(|(key, _)| *key == reference)
                            .map(|(_, index)| *index)
                    })
                {
                    return Ok(Some(page_index));
                }
                PdfReadDiagnosticCode::UnsupportedAnnotation
            }
            Err(
                PdfReadError::ObjectLimit
                | PdfReadError::TokenLimit
                | PdfReadError::NestingLimit
                | PdfReadError::StreamLimit,
            ) => PdfReadDiagnosticCode::ContentLimit,
            Err(error) => return Err(error),
        };
        self.report.push(PdfReadDiagnostic::blocked(
            code,
            Some(self.page_index),
            Some(annotation_ref.object_number),
        ));
        Ok(None)
    }

    fn field_root_count(&self, field: PdfObjectRef) -> usize {
        self.field_root_counts
            .as_ref()
            .and_then(|counts| counts.get(&field))
            .copied()
            .unwrap_or(0)
    }

    fn widget_field_owner<'workspace>(
        &mut self,
        widget: PdfObjectRef,
        dictionary: &BTreeMap<String, PdfValue>,
        workspace: &'workspace Cell<usize>,
    ) -> Result<Option<AnnotationValue<'static, 'workspace>>, PdfReadError> {
        let Some(parent_ref) = dictionary.get("Parent").and_then(PdfValue::as_reference) else {
            return Ok(None);
        };
        if parent_ref == widget
            || self.field_root_count(parent_ref) != 1
            || ["FT", "V", "Kids"]
                .iter()
                .any(|key| dictionary.contains_key(*key))
        {
            return Ok(None);
        }
        if let Some(label) = dictionary.get("T") {
            let label = resolve_annotation_value(
                label,
                self.limits.max_nesting,
                &mut self.widget_work,
                &mut |reference| self.reader.scene_label_object(reference, 0, workspace),
            )?;
            if !matches!(label.as_deref(), Some(PdfValue::Null)) {
                return Ok(None);
            }
        }
        self.widget_work = self
            .widget_work
            .checked_sub(1)
            .ok_or(PdfReadError::TokenLimit)?;
        let owner = self.reader.scene_label_object(parent_ref, 0, workspace)?;
        let Some(field) = owner.as_deref().and_then(PdfValue::as_dictionary) else {
            return Ok(None);
        };
        if field.contains_key("Parent")
            || !matches!(
                field.get("FT").and_then(PdfValue::as_name),
                Some("Tx" | "Btn")
            )
        {
            return Ok(None);
        }
        let Some(kids) = field.get("Kids") else {
            return Ok(None);
        };
        let children = resolve_annotation_value(
            kids,
            self.limits.max_nesting,
            &mut self.widget_work,
            &mut |reference| self.reader.scene_label_object(reference, 0, workspace),
        )?;
        if !matches!(children.as_deref().and_then(PdfValue::as_array), Some([PdfValue::Reference(child)]) if *child == widget)
        {
            return Ok(None);
        }
        drop(children);
        self.widget_work = self
            .widget_work
            .checked_sub(1)
            .ok_or(PdfReadError::TokenLimit)?;
        Ok(owner)
    }

    fn push_element_with_workspace(
        &mut self,
        element: PdfSceneElement,
        heap_bytes: usize,
    ) -> Result<(), PdfReadError> {
        if !self.emit_elements {
            return Ok(());
        }
        if self.elements.len() >= self.limits.max_scene_elements {
            self.report.push(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::SceneLimit,
                Some(self.page_index),
                None,
            ));
            return Err(PdfReadError::TokenLimit);
        }
        let mut encoded = self.budget;
        encoded.charge(usize::from(!self.elements.is_empty()))?;
        let heap_remaining = encoded.remaining_after(heap_bytes)?;
        let slot_bytes = size_of::<PdfSceneElement>();
        // @invariant: The occupied slot can cover part of the element's encoded charge.
        encoded.release(slot_bytes);
        encoded.encode(&element)?;
        let mut remaining = encoded.remaining_scene_bytes.min(heap_remaining);
        let old_capacity = self.elements.capacity();
        if self.elements.len() == old_capacity {
            let capacity = old_capacity
                .saturating_add((old_capacity / 2).max(1))
                .min(self.limits.max_scene_elements);
            let old_bytes = old_capacity * slot_bytes;
            let new_bytes = capacity
                .checked_mul(slot_bytes)
                .ok_or(PdfReadError::StreamLimit)?;
            // @invariant: Both buffers can coexist with the pending element's heap.
            let growth_allowance = heap_remaining.min(remaining + old_bytes);
            if new_bytes > growth_allowance {
                return Err(PdfReadError::StreamLimit);
            }
            self.elements
                .try_reserve_exact(capacity - self.elements.len())
                .map_err(|_| PdfReadError::StreamLimit)?;
            let actual_bytes = self.elements.capacity() * slot_bytes;
            if actual_bytes > growth_allowance {
                return Err(PdfReadError::StreamLimit);
            }
            remaining = remaining + old_bytes - actual_bytes;
        }
        self.budget.remaining_scene_bytes = remaining;
        self.elements.push(element);
        Ok(())
    }
}

fn copy_scene_string(
    value: Option<&str>,
    remaining: &mut usize,
) -> Result<Option<String>, PdfReadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    *remaining = remaining
        .checked_sub(value.len())
        .ok_or(PdfReadError::StreamLimit)?;
    let mut string = String::new();
    string
        .try_reserve_exact(value.len())
        .map_err(|_| PdfReadError::StreamLimit)?;
    string.push_str(value);
    Ok(Some(string))
}

fn explicit_destination_target(destination: &[PdfValue]) -> Option<PdfObjectRef> {
    let mode = destination.get(1)?.as_name()?;
    let length = match mode {
        "Fit" | "FitB" => 2,
        "FitH" | "FitV" | "FitBH" | "FitBV" => 3,
        "XYZ" => 5,
        "FitR" => 6,
        _ => return None,
    };
    if destination.len() != length
        || !destination[2..].iter().all(|value| {
            value.as_number().is_some() || (mode != "FitR" && matches!(value, PdfValue::Null))
        })
    {
        return None;
    }
    destination.first()?.as_reference()
}

#[derive(Debug, Clone, Copy)]
struct SceneBudget {
    remaining_scene_bytes: usize,
    remaining_image_hex_bytes: usize,
}

impl SceneBudget {
    fn remaining_after(&self, bytes: usize) -> Result<usize, PdfReadError> {
        self.remaining_scene_bytes
            .checked_sub(bytes)
            .ok_or(PdfReadError::StreamLimit)
    }

    fn charge(&mut self, bytes: usize) -> Result<(), PdfReadError> {
        self.remaining_scene_bytes = self.remaining_after(bytes)?;
        Ok(())
    }

    fn release(&mut self, bytes: usize) {
        self.remaining_scene_bytes += bytes;
    }

    fn reserve_vec<T>(
        &mut self,
        values: &mut Vec<T>,
        additional: usize,
    ) -> Result<(), PdfReadError> {
        reserve_scene_vec(values, additional, &mut self.remaining_scene_bytes)
    }

    fn encode(&mut self, value: &impl Serialize) -> Result<(), PdfReadError> {
        let mut writer = SceneJsonWriter {
            output: std::io::sink(),
            remaining: self.remaining_scene_bytes,
        };
        writer.encode(value)?;
        self.remaining_scene_bytes = writer.remaining;
        Ok(())
    }

    fn image_hex(&mut self, data: &[u8]) -> Result<Option<String>, PdfReadError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let Some(len) = data
            .len()
            .checked_mul(2)
            .filter(|&len| len <= self.remaining_image_hex_bytes)
        else {
            return Ok(None);
        };
        self.remaining_after(len)?;
        let mut hex = String::new();
        if hex.try_reserve_exact(len).is_err() {
            return Ok(None);
        }
        for &byte in data {
            hex.push(HEX[usize::from(byte >> 4)] as char);
            hex.push(HEX[usize::from(byte & 0x0F)] as char);
        }
        self.remaining_image_hex_bytes -= len;
        Ok(Some(hex))
    }
}

struct SceneJsonWriter<W> {
    output: W,
    remaining: usize,
}

impl<W: std::io::Write> SceneJsonWriter<W> {
    fn encode(&mut self, value: &impl Serialize) -> Result<(), PdfReadError> {
        serde_json::to_writer(self, value).map_err(|error| {
            if error.io_error_kind() == Some(std::io::ErrorKind::FileTooLarge) {
                PdfReadError::StreamLimit
            } else {
                PdfReadError::Syntax
            }
        })
    }
}

impl<W: std::io::Write> std::io::Write for SceneJsonWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::ErrorKind::FileTooLarge.into());
        }
        let written = self.output.write(bytes)?;
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

fn scene_form_entry<'a, 'workspace>(
    reader: &PdfReader,
    value: &'a PdfValue,
    depth: usize,
    workspace: &'workspace Cell<usize>,
) -> Result<Option<AnnotationValue<'a, 'workspace>>, PdfReadError> {
    if depth > reader.limits().max_nesting {
        return Err(PdfReadError::NestingLimit);
    }
    match value.as_reference() {
        Some(reference) => reader.scene_label_object(reference, depth + 1, workspace),
        None => Ok(Some(AnnotationValue::borrowed(value))),
    }
}

fn read_field_root_counts(
    reader: &PdfReader,
    workspace: &Cell<usize>,
) -> Result<BTreeMap<PdfObjectRef, usize>, PdfReadError> {
    use std::collections::btree_map::Entry;

    let mut counts = BTreeMap::new();
    let catalog = reader.scene_non_stream_object(reader.root_for_scene(), 0, workspace)?;
    let dictionary = catalog
        .as_dictionary()
        .ok_or(PdfReadError::PageTreeInvalid)?;
    if let Some(form) = dictionary.get("AcroForm") {
        let form = scene_form_entry(reader, form, 1, workspace)?;
        if let Some(fields) = form
            .as_deref()
            .and_then(PdfValue::as_dictionary)
            .and_then(|form| form.get("Fields"))
        {
            let fields = scene_form_entry(reader, fields, 2, workspace)?;
            if let Some(fields) = fields.as_deref().and_then(PdfValue::as_array) {
                for field in fields.iter().filter_map(PdfValue::as_reference) {
                    match counts.entry(field) {
                        Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                        Entry::Vacant(entry) => {
                            charge_scene_workspace(
                                workspace,
                                scene_tree_entry_bytes::<(PdfObjectRef, usize)>(),
                            )?;
                            entry.insert(1);
                        }
                    }
                }
            }
        }
    }
    Ok(counts)
}

/// Reads a bounded page window and calculates a stable scene hash.
pub fn read_pdf_scene(
    reader: &PdfReader,
    request: &PdfSceneRequest,
) -> Result<PdfScene, PdfReadError> {
    let scene_byte_limit = reader.limits().max_scene_bytes;
    let workspace = Cell::new(scene_byte_limit);
    let catalog = reader.catalog_without_form(&workspace)?;
    let pages_ref = PdfReader::page_tree_root(&catalog)?;
    let dictionary = catalog
        .as_dictionary()
        .ok_or(PdfReadError::PageTreeInvalid)?;
    let mut report = reader.preflight_report();
    report_active_content(
        dictionary,
        None,
        Some(reader.root_for_scene().object_number),
        &mut report,
    );
    let mut field_root_counts = (!dictionary.contains_key("AcroForm")).then(BTreeMap::new);
    drop(catalog);
    let pages = reader.page_records_from_root(pages_ref, Some(&workspace))?;
    let page_count = u32::try_from(pages.len()).map_err(|_| PdfReadError::PageCountLimit)?;
    let first = usize::try_from(request.first_page).map_err(|_| PdfReadError::PageCountLimit)?;
    let count = usize::try_from(request.page_count).map_err(|_| PdfReadError::PageCountLimit)?;
    if first > pages.len() || count > reader.limits().max_pages {
        return Err(PdfReadError::PageCountLimit);
    }
    let end = first
        .checked_add(count)
        .ok_or(PdfReadError::PageCountLimit)?
        .min(pages.len());
    // @invariant: Retained elements and image hex share allowances across the requested page window.
    let mut budget = SceneBudget {
        remaining_scene_bytes: workspace.into_inner(),
        remaining_image_hex_bytes: reader.limits().max_stream_bytes,
    };
    let mut page_refs = Vec::new();
    if pages[first..end]
        .iter()
        .any(|page| !page.annotations.is_empty())
    {
        budget.reserve_vec(&mut page_refs, pages.len())?;
        page_refs.extend(pages.iter().map(|page| page.object_ref).zip(0..page_count));
        page_refs.sort_unstable();
    }
    let mut scene_pages = Vec::with_capacity(end.saturating_sub(first));
    for (index, page) in pages.iter().enumerate().take(end).skip(first) {
        let page_index = u32::try_from(index).map_err(|_| PdfReadError::PageCountLimit)?;
        let had_field_root_counts = field_root_counts.is_some();
        match SceneBuilder::new(
            reader,
            &page_refs,
            page_index,
            page,
            &mut field_root_counts,
            budget,
        )
        .run()
        {
            Ok((page, remaining_budget)) => {
                budget = remaining_budget;
                report.extend(&page.report, |bytes| budget.charge(bytes))?;
                scene_pages.push(page);
            }
            Err(PdfReadError::FilterUnsupported) => {
                if !had_field_root_counts {
                    field_root_counts = None;
                }
                report.push(PdfReadDiagnostic::blocked(
                    PdfReadDiagnosticCode::UnsupportedFilter,
                    Some(page_index),
                    None,
                ));
            }
            Err(error) => return Err(error),
        }
    }
    drop(page_refs);
    drop(pages);
    drop(field_root_counts);
    let mut scene = PdfScene {
        schema_version: PDF_SCENE_SCHEMA_VERSION,
        source_hash: reader.source_hash().to_owned(),
        page_count,
        first_page: request.first_page,
        pages: scene_pages,
        report,
        result_hash: String::new(),
    };
    let mut hasher = SceneJsonWriter {
        output: blake3::Hasher::new(),
        remaining: scene_byte_limit,
    };
    hasher.encode(&scene)?;
    scene.result_hash = hasher.output.finalize().to_hex().to_string();
    Ok(scene)
}

fn is_known_noop(operator: &str) -> bool {
    matches!(
        operator,
        "J" | "j"
            | "ri"
            | "i"
            | "CS"
            | "cs"
            | "SC"
            | "SCN"
            | "sc"
            | "scn"
            | "G"
            | "g"
            | "RG"
            | "rg"
            | "K"
            | "k"
            | "sh"
            | "BX"
            | "EX"
            | "Do"
    )
}

impl ContentToken {
    fn precise_number(&self) -> f64 {
        self.scene_decimal().value()
    }

    fn scene_decimal(&self) -> SceneDecimal {
        match self {
            Self::Number((_, value)) => *value,
            Self::Integer(value) => SceneDecimal::Exact {
                coefficient: *value,
                scale: 0,
            },
            _ => unreachable!("validated numeric operand"),
        }
    }

    fn number(&self) -> Option<LayoutUnit> {
        match self {
            Self::Number((value, _)) => Some(*value),
            Self::Integer(value) => value
                .checked_mul(LayoutUnit::UNITS_PER_POINT)
                .map(LayoutUnit::from_raw),
            _ => None,
        }
    }
}

fn font_width(value: &PdfValue) -> Option<LayoutUnit> {
    match value {
        PdfValue::Integer(value) => Some(LayoutUnit::from_raw(*value)),
        PdfValue::Real((value, _)) => Some(*value),
        _ => None,
    }
}

fn one_number(values: &[ContentToken]) -> Result<LayoutUnit, PdfReadError> {
    values
        .last()
        .and_then(ContentToken::number)
        .ok_or(PdfReadError::Syntax)
}

fn precise_operand(values: &[ContentToken]) -> Option<f64> {
    let value = values.last()?;
    value.number()?;
    Some(value.precise_number())
}

fn two_numbers(values: &[ContentToken]) -> Result<(LayoutUnit, LayoutUnit), PdfReadError> {
    if values.len() < 2 {
        return Err(PdfReadError::Syntax);
    }
    Ok((
        values[values.len() - 2]
            .number()
            .ok_or(PdfReadError::Syntax)?,
        values[values.len() - 1]
            .number()
            .ok_or(PdfReadError::Syntax)?,
    ))
}

fn four_number_values(values: &[ContentToken]) -> Result<[LayoutUnit; 4], PdfReadError> {
    if values.len() < 4 {
        return Err(PdfReadError::Syntax);
    }
    Ok([
        values[values.len() - 4]
            .number()
            .ok_or(PdfReadError::Syntax)?,
        values[values.len() - 3]
            .number()
            .ok_or(PdfReadError::Syntax)?,
        values[values.len() - 2]
            .number()
            .ok_or(PdfReadError::Syntax)?,
        values[values.len() - 1]
            .number()
            .ok_or(PdfReadError::Syntax)?,
    ])
}

fn six_numbers(values: &[ContentToken]) -> Option<Matrix> {
    if values.len() < 6 {
        return None;
    }
    Some(Matrix {
        a: values[values.len() - 6].number()?,
        b: values[values.len() - 5].number()?,
        c: values[values.len() - 4].number()?,
        d: values[values.len() - 3].number()?,
        e: values[values.len() - 2].number()?,
        f: values[values.len() - 1].number()?,
    })
}

fn six_number_values(values: &[ContentToken]) -> Result<[LayoutUnit; 6], PdfReadError> {
    let matrix = six_numbers(values).ok_or(PdfReadError::Syntax)?;
    Ok([matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f])
}

fn transform_path(
    commands: &[RawPathCommand],
    matrix: Matrix,
    close: bool,
    mut workspace: SceneBudget,
) -> Result<Vec<PdfPathCommand>, PdfReadError> {
    let mut transformed = Vec::new();
    workspace.reserve_vec(&mut transformed, commands.len() + usize::from(close))?;
    for command in commands {
        transformed.push(match command {
            RawPathCommand::MoveTo(x, y, _) => {
                let (x, y) = matrix.point(*x, *y)?;
                PdfPathCommand::MoveTo { x, y }
            }
            RawPathCommand::LineTo(x, y, _) => {
                let (x, y) = matrix.point(*x, *y)?;
                PdfPathCommand::LineTo { x, y }
            }
            RawPathCommand::CurveTo(x1, y1, x2, y2, x3, y3, _) => {
                let (x1, y1) = matrix.point(*x1, *y1)?;
                let (x2, y2) = matrix.point(*x2, *y2)?;
                let (x3, y3) = matrix.point(*x3, *y3)?;
                PdfPathCommand::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                }
            }
            RawPathCommand::Close => PdfPathCommand::ClosePath,
        });
    }
    if close {
        transformed.push(PdfPathCommand::ClosePath);
    }
    Ok(transformed)
}

fn has_noncollinear_points<T: Copy>(
    mut points: impl Iterator<Item = [T; 2]>,
    same_point: impl Fn([T; 2], [T; 2]) -> bool,
    is_collinear: impl Fn([T; 2], [T; 2], [T; 2]) -> bool,
) -> bool {
    let Some(first) = points.next() else {
        return false;
    };
    let Some(second) = points.find(|&point| !same_point(point, first)) else {
        return false;
    };
    points.any(|point| {
        !same_point(point, first)
            && !same_point(point, second)
            && !is_collinear(first, second, point)
    })
}

fn align_decimal_axis<const N: usize>(axis: [SceneDecimal; N]) -> Option<[i128; N]> {
    let scale = axis
        .iter()
        .filter_map(|value| match value {
            SceneDecimal::Exact { scale, .. } => Some(*scale),
            SceneDecimal::Approximate(_) => None,
        })
        .max()?;
    let mut aligned = [0; N];
    for (target, value) in aligned.iter_mut().zip(axis) {
        let SceneDecimal::Exact {
            coefficient,
            scale: source_scale,
        } = value
        else {
            return None;
        };
        *target =
            i128::from(coefficient).checked_mul(10_i128.checked_pow(scale - source_scale)?)?;
    }
    Some(aligned)
}

fn same_decimal_point([x, y]: [SceneDecimal; 2], [a, b]: [SceneDecimal; 2]) -> bool {
    [[x, a], [y, b]]
        .into_iter()
        .all(|axis| align_decimal_axis(axis).is_some_and(|[first, second]| first == second))
}

fn decimal_points_collinear(
    [x, y]: [SceneDecimal; 2],
    [a, b]: [SceneDecimal; 2],
    [c, d]: [SceneDecimal; 2],
) -> Option<bool> {
    let [x, a, c] = align_decimal_axis([x, a, c])?;
    let [y, b, d] = align_decimal_axis([y, b, d])?;
    Some(
        a.checked_sub(x)?.checked_mul(d.checked_sub(y)?)?
            == b.checked_sub(y)?.checked_mul(c.checked_sub(x)?)?,
    )
}

fn lost_visible_fill_contour(
    source: &[RawPathCommand],
    commands: &[PdfPathCommand],
    matrix: Option<PaintMatrix>,
    clip: PaintBounds,
) -> bool {
    if clip.is_empty() {
        return false;
    }
    let mut start = 0;
    let mut origin = None;
    while start < source.len() {
        let explicit = matches!(source[start], RawPathCommand::MoveTo(..));
        if explicit {
            origin = Some(start);
        }
        let implicit_origin = origin.filter(|_| !explicit);
        let end = source[start..]
            .iter()
            .enumerate()
            .find_map(|(offset, command)| match command {
                RawPathCommand::MoveTo(..) if offset != 0 => Some(start + offset),
                RawPathCommand::Close => Some(start + offset + 1),
                _ => None,
            })
            .unwrap_or(source.len());
        let indices = implicit_origin.into_iter().chain(start..end);
        let emitted = indices.clone().flat_map(|index| {
            let raw = |x: LayoutUnit, y: LayoutUnit| [i128::from(x.raw()), i128::from(y.raw())];
            match commands[index] {
                PdfPathCommand::MoveTo { x, y } | PdfPathCommand::LineTo { x, y } => {
                    [Some(raw(x, y)), None, None]
                }
                PdfPathCommand::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                } => [Some(raw(x1, y1)), Some(raw(x2, y2)), Some(raw(x3, y3))],
                PdfPathCommand::ClosePath => [None; 3],
            }
            .into_iter()
            .flatten()
        });
        let precise = indices
            .flat_map(|index| match &source[index] {
                RawPathCommand::MoveTo(_, _, point) | RawPathCommand::LineTo(_, _, point) => {
                    std::slice::from_ref(point)
                }
                RawPathCommand::CurveTo(_, _, _, _, _, _, points) => points,
                RawPathCommand::Close => &[],
            })
            .copied();
        start = end;
        let product =
            |a: i128, b: i128| (a.signum() * b.signum(), a.unsigned_abs() * b.unsigned_abs());
        if has_noncollinear_points(
            emitted,
            |a, b| a == b,
            |[x, y], [a, b], [c, d]| product(a - x, d - y) == product(b - y, c - x),
        ) || !has_noncollinear_points(precise.clone(), same_decimal_point, |a, b, c| {
            decimal_points_collinear(a, b, c) == Some(true)
        }) {
            continue;
        }
        let bounds = precise
            .map(|point| point.map(SceneDecimal::value))
            .map(|[x, y]| PaintBounds([x, y, x, y]))
            .reduce(PaintBounds::union);
        if matrix
            .and_then(|matrix| matrix.bounds(bounds?.0))
            // @invariant: A rounded point or line bound cannot by itself prove clipping.
            .and_then(|bounds| {
                if bounds.is_empty() {
                    bounds.expanded(0.)
                } else {
                    Some(bounds)
                }
            })
            .is_none_or(|bounds| !clip.intersect(bounds).is_empty())
        {
            return true;
        }
    }
    false
}

fn contains_collapsed_stroke(commands: &[PdfPathCommand], source: &[RawPathCommand]) -> bool {
    let mut current = (None, None);
    let mut start = current;
    for (index, command) in commands.iter().enumerate() {
        let precise = match source.get(index) {
            Some(RawPathCommand::MoveTo(_, _, point) | RawPathCommand::LineTo(_, _, point)) => {
                Some(point.map(SceneDecimal::value))
            }
            Some(RawPathCommand::CurveTo(_, _, _, _, _, _, points)) => {
                Some(points[2].map(SceneDecimal::value))
            }
            _ => None,
        };
        match *command {
            PdfPathCommand::MoveTo { x, y } => {
                start = (Some((x, y)), precise);
                current = start;
            }
            PdfPathCommand::LineTo { x, y } => {
                let next = Some((x, y));
                if current.0 == next {
                    return true;
                }
                current = (next, precise);
            }
            PdfPathCommand::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                let next = Some((x3, y3));
                if current.0 == next && (x1, y1) == (x3, y3) && (x2, y2) == (x3, y3) {
                    return true;
                }
                current = (next, precise);
            }
            PdfPathCommand::ClosePath => {
                if current.0 == start.0
                    && matches!((current.1, start.1), (Some(end), Some(first)) if end != first)
                {
                    return true;
                }
                current = start;
            }
        }
    }
    false
}

fn path_bounds(commands: &[PdfPathCommand]) -> Result<LayoutRect, PdfReadError> {
    let mut bounds = None::<[i64; 4]>;
    let mut include = |x: LayoutUnit, y: LayoutUnit| {
        let (x, y) = (x.raw(), y.raw());
        bounds = Some(match bounds {
            Some([min_x, min_y, max_x, max_y]) => {
                [min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y)]
            }
            None => [x, y, x, y],
        });
    };
    for command in commands {
        match *command {
            PdfPathCommand::MoveTo { x, y } | PdfPathCommand::LineTo { x, y } => include(x, y),
            PdfPathCommand::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => {
                include(x1, y1);
                include(x2, y2);
                include(x3, y3);
            }
            PdfPathCommand::ClosePath => {}
        }
    }
    let [min_x, min_y, max_x, max_y] = bounds.unwrap_or([0; 4]);
    Ok(LayoutRect {
        x: LayoutUnit::from_raw(min_x),
        y: LayoutUnit::from_raw(min_y),
        width: LayoutUnit::from_raw(max_x.checked_sub(min_x).ok_or(PdfReadError::NumericLimit)?),
        height: LayoutUnit::from_raw(max_y.checked_sub(min_y).ok_or(PdfReadError::NumericLimit)?),
    })
}

fn union_rect(mut rects: impl Iterator<Item = LayoutRect>) -> Result<LayoutRect, PdfReadError> {
    let first = rects.next().ok_or(PdfReadError::NumericLimit)?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x.checked_add(first.width)?;
    let mut max_y = first.y.checked_add(first.height)?;
    for rect in rects {
        min_x = min_x.min(rect.x);
        min_y = min_y.min(rect.y);
        max_x = max_x.max(rect.x.checked_add(rect.width)?);
        max_y = max_y.max(rect.y.checked_add(rect.height)?);
    }
    Ok(LayoutRect {
        x: min_x,
        y: min_y,
        width: max_x.checked_sub(min_x)?,
        height: max_y.checked_sub(min_y)?,
    })
}

fn transformed_rect(matrix: Matrix, rect: LayoutRect) -> Result<LayoutRect, PdfReadError> {
    let right = rect.x.checked_add(rect.width)?;
    let top = rect.y.checked_add(rect.height)?;
    let (x0, y0) = matrix.point(rect.x, rect.y)?;
    let (x1, y1) = matrix.point(right, rect.y)?;
    let (x2, y2) = matrix.point(rect.x, top)?;
    let (x3, y3) = matrix.point(right, top)?;
    let x = x0.min(x1).min(x2).min(x3);
    let y = y0.min(y1).min(y2).min(y3);
    Ok(LayoutRect {
        x,
        y,
        width: x0.max(x1).max(x2).max(x3).checked_sub(x)?,
        height: y0.max(y1).max(y2).max(y3).checked_sub(y)?,
    })
}

fn is_rectangular_path(commands: &[PdfPathCommand]) -> bool {
    let commands = match commands {
        [commands @ .., PdfPathCommand::ClosePath] => commands,
        _ => commands,
    };
    let [
        PdfPathCommand::MoveTo { x: x0, y: y0 },
        PdfPathCommand::LineTo { x: x1, y: y1 },
        PdfPathCommand::LineTo { x: x2, y: y2 },
        PdfPathCommand::LineTo { x: x3, y: y3 },
        closing @ ..,
    ] = commands
    else {
        return false;
    };
    if !closing.is_empty()
        && !matches!(closing, [PdfPathCommand::LineTo { x, y }] if x == x0 && y == y0)
    {
        return false;
    }
    (x0 == x3 && x1 == x2 && y0 == y1 && y2 == y3) || (x0 == x1 && x2 == x3 && y0 == y3 && y1 == y2)
}

fn is_precise_rectangular_path(commands: &[RawPathCommand], matrix: PaintMatrix) -> bool {
    let commands = match commands {
        [commands @ .., RawPathCommand::Close] => commands,
        _ => commands,
    };
    let [
        RawPathCommand::MoveTo(_, _, p0),
        RawPathCommand::LineTo(_, _, p1),
        RawPathCommand::LineTo(_, _, p2),
        RawPathCommand::LineTo(_, _, p3),
        closing @ ..,
    ] = commands
    else {
        return false;
    };
    if !closing.is_empty()
        && !matches!(closing, [RawPathCommand::LineTo(_, _, point)] if point.map(SceneDecimal::value) == p0.map(SceneDecimal::value))
    {
        return false;
    }
    let PaintMatrix([a, b, c, d, ..]) = matrix;
    // @invariant: Translation cannot round skew out of edge vectors.
    let edge = |[x0, y0]: [f64; 2], [x1, y1]: [f64; 2]| {
        let dx = x1 - x0;
        let dy = y1 - y0;
        [a * dx + c * dy, b * dx + d * dy]
    };
    let edges = [
        edge(p0.map(SceneDecimal::value), p1.map(SceneDecimal::value)),
        edge(p1.map(SceneDecimal::value), p2.map(SceneDecimal::value)),
        edge(p2.map(SceneDecimal::value), p3.map(SceneDecimal::value)),
        edge(p3.map(SceneDecimal::value), p0.map(SceneDecimal::value)),
    ];
    let horizontal = |[x, y]: [f64; 2]| x.is_finite() && x != 0. && y == 0.;
    let vertical = |[x, y]: [f64; 2]| x == 0. && y.is_finite() && y != 0.;
    let [first, second, third, fourth] = edges;
    (horizontal(first) && vertical(second) && horizontal(third) && vertical(fourth))
        || (vertical(first) && horizontal(second) && vertical(third) && horizontal(fourth))
}

fn parse_bbox(value: &PdfValue) -> Result<LayoutRect, PdfReadError> {
    let values = value.as_array().ok_or(PdfReadError::NumericLimit)?;
    if values.len() != 4 {
        return Err(PdfReadError::NumericLimit);
    }
    let x = values[0].as_number().ok_or(PdfReadError::NumericLimit)?;
    let y = values[1].as_number().ok_or(PdfReadError::NumericLimit)?;
    let x1 = values[2].as_number().ok_or(PdfReadError::NumericLimit)?;
    let y1 = values[3].as_number().ok_or(PdfReadError::NumericLimit)?;
    Ok(LayoutRect {
        x,
        y,
        width: x1.checked_sub(x)?,
        height: y1.checked_sub(y)?,
    })
}

fn next_code<'a>(remaining: &'a [u8], mapping: &BTreeMap<Vec<u8>, String>) -> (&'a [u8], u8) {
    let width = mapping
        .keys()
        .filter(|key| !key.is_empty() && remaining.starts_with(key))
        .map(Vec::len)
        .max()
        .unwrap_or(1);
    let code_bytes = &remaining[..width];
    let code = code_bytes
        .iter()
        .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte));
    (code_bytes, u8::try_from(code).unwrap_or(0))
}

fn parse_to_unicode(
    bytes: &[u8],
    max_entries: usize,
    budget: &mut SceneBudget,
) -> Result<BTreeMap<Vec<u8>, String>, PdfReadError> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => {
            let capacity = bytes.utf8_chunks().try_fold(0_usize, |total, chunk| {
                total
                    .checked_add(chunk.valid().len())?
                    .checked_add(3 * usize::from(!chunk.invalid().is_empty()))
            });
            let capacity = capacity.ok_or(PdfReadError::StreamLimit)?;
            budget.charge(capacity)?;
            let mut text = String::with_capacity(capacity);
            for chunk in bytes.utf8_chunks() {
                text.push_str(chunk.valid());
                if !chunk.invalid().is_empty() {
                    text.push('\u{FFFD}');
                }
            }
            Cow::Owned(text)
        }
    };
    let mut tokens = text.split_whitespace();
    let mut map = BTreeMap::new();
    while let Some(token) = tokens.next() {
        if token == "beginbfchar" {
            while let Some(source) = tokens.next() {
                if source == "endbfchar" {
                    break;
                }
                let source = HexToken::new(source)?;
                let destination = HexToken::new(tokens.next().ok_or(PdfReadError::Syntax)?)?;
                budget.charge(source.decoded_len())?;
                let source = source.to_bytes();
                let destination = unicode_destination(destination, budget)?;
                insert_unicode(&mut map, source, destination, max_entries, budget)?;
            }
        } else if token == "beginbfrange" {
            while let Some(low) = tokens.next() {
                if low == "endbfrange" {
                    break;
                }
                let low = HexToken::new(low)?;
                let high = HexToken::new(tokens.next().ok_or(PdfReadError::Syntax)?)?;
                let destination = tokens.next().ok_or(PdfReadError::Syntax)?;
                if destination.starts_with('[') {
                    continue;
                }
                let low_value = low.numeric_value();
                let high_value = high.numeric_value();
                let destination_start = HexToken::new(destination)?.numeric_value();
                let width = low.decoded_len();
                for value in low_value..=high_value {
                    let codepoint = destination_start
                        .checked_add(value - low_value)
                        .ok_or(PdfReadError::NumericLimit)?;
                    let scalar = char::from_u32(codepoint).unwrap_or('\u{FFFD}');
                    budget.charge(width)?;
                    budget.charge(scalar.len_utf8())?;
                    let mut source = vec![0; width];
                    let bytes = value.to_be_bytes();
                    let copied_width = width.min(bytes.len());
                    source[width - copied_width..]
                        .copy_from_slice(&bytes[bytes.len() - copied_width..]);
                    let mut destination = String::with_capacity(scalar.len_utf8());
                    destination.push(scalar);
                    insert_unicode(&mut map, source, destination, max_entries, budget)?;
                }
            }
        }
    }
    if let Cow::Owned(text) = text {
        let capacity = text.capacity();
        drop(text);
        budget.release(capacity);
    }
    Ok(map)
}

struct HexToken<'a>(&'a [u8]);

impl<'a> HexToken<'a> {
    fn new(token: &'a str) -> Result<Self, PdfReadError> {
        match token.as_bytes() {
            [b'<', digits @ .., b'>'] if digits.iter().all(|&byte| hex_nibble(byte).is_some()) => {
                Ok(Self(digits))
            }
            _ => Err(PdfReadError::Syntax),
        }
    }

    fn decoded_len(&self) -> usize {
        self.0.len().div_ceil(2)
    }

    fn bytes(&self) -> impl Iterator<Item = u8> + 'a {
        self.0.chunks(2).map(|pair| {
            let high = hex_nibble(pair[0]).unwrap();
            let low = pair.get(1).and_then(|&byte| hex_nibble(byte)).unwrap_or(0);
            (high << 4) | low
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.decoded_len());
        bytes.extend(self.bytes());
        bytes
    }

    fn numeric_value(&self) -> u32 {
        self.bytes()
            .fold(0, |value, byte| (value << 8) | u32::from(byte))
    }

    fn utf16(&self) -> impl Iterator<Item = Result<char, std::char::DecodeUtf16Error>> + 'a {
        let mut bytes = self.bytes();
        char::decode_utf16(std::iter::from_fn(move || {
            Some(u16::from_be_bytes([bytes.next()?, bytes.next()?]))
        }))
    }
}

fn unicode_destination(
    token: HexToken<'_>,
    budget: &mut SceneBudget,
) -> Result<String, PdfReadError> {
    if !token.decoded_len().is_multiple_of(2) {
        return Err(PdfReadError::Syntax);
    }
    let capacity = token.utf16().try_fold(0_usize, |total, scalar| {
        let scalar = scalar.map_err(|_| PdfReadError::Syntax)?;
        total
            .checked_add(scalar.len_utf8())
            .ok_or(PdfReadError::StreamLimit)
    })?;
    budget.charge(capacity)?;
    let mut destination = String::with_capacity(capacity);
    for scalar in token.utf16() {
        destination.push(scalar.map_err(|_| PdfReadError::Syntax)?);
    }
    Ok(destination)
}

fn insert_unicode(
    map: &mut BTreeMap<Vec<u8>, String>,
    source: Vec<u8>,
    destination: String,
    max_entries: usize,
    budget: &mut SceneBudget,
) -> Result<(), PdfReadError> {
    if let Some(existing) = map.get_mut(source.as_slice()) {
        let released = source.capacity() + existing.capacity();
        drop(source);
        *existing = destination;
        budget.release(released);
    } else {
        if map.len() >= max_entries {
            return Err(PdfReadError::ObjectLimit);
        }
        budget.charge(scene_tree_entry_bytes::<(Vec<u8>, String)>())?;
        map.insert(source, destination);
    }
    Ok(())
}

pub(super) fn separated_content_streams(previous: &[u8], next: &[u8]) -> bool {
    previous.is_empty()
        || matches!(previous.last(), Some(b'\n' | b'\r'))
        || matches!(next.first(), Some(b'\n' | b'\r'))
}

enum ContentParseError {
    Read(PdfReadError),
    OperatorSyntax,
    TruncatedOperand,
}

impl From<PdfReadError> for ContentParseError {
    fn from(error: PdfReadError) -> Self {
        Self::Read(error)
    }
}

impl From<ContentParseError> for PdfReadError {
    fn from(error: ContentParseError) -> Self {
        match error {
            ContentParseError::Read(error) => error,
            ContentParseError::OperatorSyntax | ContentParseError::TruncatedOperand => Self::Syntax,
        }
    }
}

pub(super) struct ContentCursor<'a> {
    bytes: &'a [u8],
    position: usize,
    limits: PdfReaderLimits,
    remaining_tokens: usize,
    array_depth: usize,
    open_comment: bool,
    workspace: Option<SceneBudget>,
}

impl<'a> ContentCursor<'a> {
    pub(super) fn new(bytes: &'a [u8], limits: PdfReaderLimits) -> Self {
        Self {
            bytes,
            position: 0,
            remaining_tokens: limits.max_content_tokens,
            limits,
            array_depth: 0,
            open_comment: false,
            workspace: None,
        }
    }

    pub(super) fn position(&self) -> usize {
        self.position
    }

    pub(super) fn next(&mut self) -> Result<Option<(ContentToken, usize)>, PdfReadError> {
        let mut remaining = self.remaining_tokens;
        let result = self.next_token(&mut remaining);
        self.remaining_tokens = remaining;
        result.map_err(PdfReadError::from)
    }

    fn next_scene_token(
        &mut self,
        remaining: &mut usize,
        budget: &mut SceneBudget,
    ) -> Result<Option<(ContentToken, usize, usize)>, ContentParseError> {
        self.workspace = Some(*budget);
        let result = self.next_token(remaining);
        let workspace = self.workspace.take().unwrap();
        // @invariant: Only a successfully returned token transfers live storage to the caller.
        let token = result?;
        let bytes = budget.remaining_scene_bytes - workspace.remaining_scene_bytes;
        *budget = workspace;
        Ok(token.map(|(token, offset)| (token, offset, bytes)))
    }

    fn charge_payload(&mut self, bytes: usize) -> Result<(), PdfReadError> {
        if let Some(workspace) = &mut self.workspace {
            workspace.charge(bytes)?;
        }
        Ok(())
    }

    fn next_token(
        &mut self,
        remaining: &mut usize,
    ) -> Result<Option<(ContentToken, usize)>, ContentParseError> {
        self.skip_space();
        if self.position >= self.bytes.len() {
            return Ok(None);
        }
        *remaining = remaining.checked_sub(1).ok_or(PdfReadError::TokenLimit)?;
        let offset = self.position;
        let token = match self.bytes[self.position] {
            b'(' => ContentToken::String(self.literal_string()?),
            b'<' => {
                if self.bytes.get(self.position + 1) == Some(&b'<') {
                    return Err(PdfReadError::Syntax.into());
                }
                ContentToken::String(self.hex_string()?)
            }
            b'[' => {
                if self.array_depth >= self.limits.max_nesting {
                    return Err(PdfReadError::TokenLimit.into());
                }
                self.array_depth += 1;
                let result = self.array(remaining);
                self.array_depth -= 1;
                result?
            }
            b'/' => ContentToken::Name(self.name()?),
            _ => {
                let word = self.word();
                if word
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'+' | b'-'))
                {
                    ContentToken::Integer(parse_signed_i64(word)?)
                } else if word
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'+' | b'-' | b'.'))
                    && word.contains(&b'.')
                {
                    ContentToken::Number(parse_decimal(word)?)
                } else {
                    let word = std::str::from_utf8(word).map_err(|_| PdfReadError::Syntax)?;
                    self.charge_payload(word.len())?;
                    ContentToken::Word(word.to_owned())
                }
            }
        };
        Ok(Some((token, offset)))
    }

    fn skip_space(&mut self) {
        loop {
            while self
                .bytes
                .get(self.position)
                .is_some_and(|byte| is_content_whitespace(*byte))
            {
                self.position += 1;
            }
            if self.bytes.get(self.position) != Some(&b'%') {
                return;
            }
            while self.position < self.bytes.len()
                && !matches!(self.bytes[self.position], b'\r' | b'\n')
            {
                self.position += 1;
            }
            self.open_comment = self.position == self.bytes.len();
        }
    }

    fn word(&mut self) -> &'a [u8] {
        let start = self.position;
        while self.position < self.bytes.len() && !is_content_delimiter(self.bytes[self.position]) {
            self.position += 1;
        }
        &self.bytes[start..self.position]
    }

    fn name(&mut self) -> Result<String, PdfReadError> {
        self.position += 1;
        if let Some(workspace) = &mut self.workspace {
            return read_scene_name(self.bytes, &mut self.position, |bytes| {
                workspace.charge(bytes)
            });
        }
        std::str::from_utf8(self.word())
            .map(str::to_owned)
            .map_err(|_| PdfReadError::Syntax)
    }

    fn literal_string(&mut self) -> Result<Vec<u8>, ContentParseError> {
        self.position += 1;
        let mut output = Vec::new();
        let mut nesting = 1_usize;
        while self.position < self.bytes.len() {
            let byte = self.bytes[self.position];
            self.position += 1;
            let decoded = match byte {
                b'(' => {
                    nesting += 1;
                    Some(byte)
                }
                b')' => {
                    nesting = nesting.checked_sub(1).ok_or(PdfReadError::Syntax)?;
                    if nesting == 0 {
                        return Ok(output);
                    }
                    Some(byte)
                }
                b'\\' => {
                    let escaped = self
                        .bytes
                        .get(self.position)
                        .copied()
                        .ok_or(ContentParseError::TruncatedOperand)?;
                    self.position += 1;
                    match escaped {
                        b'n' => Some(b'\n'),
                        b'r' => Some(b'\r'),
                        b't' => Some(b'\t'),
                        b'b' => Some(8),
                        b'f' => Some(12),
                        b'\r' => {
                            if self.bytes.get(self.position) == Some(&b'\n') {
                                self.position += 1;
                            }
                            None
                        }
                        b'\n' => None,
                        b'0'..=b'7' => {
                            let mut value = escaped - b'0';
                            for _ in 0..2 {
                                let Some(next) = self.bytes.get(self.position).copied() else {
                                    break;
                                };
                                if !(b'0'..=b'7').contains(&next) {
                                    break;
                                }
                                self.position += 1;
                                value = value.wrapping_mul(8).wrapping_add(next - b'0');
                            }
                            Some(value)
                        }
                        other => Some(other),
                    }
                }
                b'\r' => {
                    if self.bytes.get(self.position) == Some(&b'\n') {
                        self.position += 1;
                    }
                    Some(b'\n')
                }
                other => Some(other),
            };
            if let Some(byte) = decoded {
                if output.len() >= self.limits.max_string_bytes {
                    return Err(PdfReadError::StreamLimit.into());
                }
                if let Some(workspace) = &mut self.workspace {
                    workspace.reserve_vec(&mut output, 1)?;
                }
                output.push(byte);
            }
        }
        Err(ContentParseError::TruncatedOperand)
    }

    fn hex_string(&mut self) -> Result<Vec<u8>, ContentParseError> {
        let start = self.position;
        self.position += 1;
        while let Some(&byte) = self.bytes.get(self.position) {
            self.position += 1;
            if byte == b'>' {
                let token = std::str::from_utf8(&self.bytes[start..self.position])
                    .map_err(|_| PdfReadError::Syntax)?;
                let token = HexToken::new(token)?;
                self.charge_payload(token.decoded_len())?;
                return Ok(token.to_bytes());
            }
            if !byte.is_ascii_hexdigit() {
                return Err(PdfReadError::Syntax.into());
            }
        }
        Err(ContentParseError::TruncatedOperand)
    }

    fn array(&mut self, remaining: &mut usize) -> Result<ContentToken, ContentParseError> {
        self.position += 1;
        let mut values = Vec::new();
        loop {
            self.skip_space();
            match self.bytes.get(self.position) {
                Some(b']') => {
                    self.position += 1;
                    return Ok(ContentToken::Array(values));
                }
                None => return Err(ContentParseError::TruncatedOperand),
                _ if values.len() >= self.limits.max_array_items => {
                    return Err(PdfReadError::TokenLimit.into());
                }
                _ => {}
            }
            let (value, _) = self
                .next_token(remaining)?
                .ok_or(ContentParseError::TruncatedOperand)?;
            if let Some(workspace) = &mut self.workspace {
                workspace.reserve_vec(&mut values, 1)?;
            }
            values.push(value);
        }
    }
}

fn is_content_delimiter(byte: u8) -> bool {
    is_content_whitespace(byte)
        || matches!(byte, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'/' | b'%')
}

fn is_content_whitespace(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 12 | b'\r' | b' ')
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_signed_i64(bytes: &[u8]) -> Result<i64, PdfReadError> {
    if bytes.is_empty() {
        return Err(PdfReadError::NumericLimit);
    }
    let negative = bytes[0] == b'-';
    let start = usize::from(bytes[0] == b'+' || negative);
    if start == bytes.len() || bytes[start..].iter().any(|byte| !byte.is_ascii_digit()) {
        return Err(PdfReadError::NumericLimit);
    }
    let mut value = 0_i128;
    for byte in &bytes[start..] {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(byte - b'0')))
            .ok_or(PdfReadError::NumericLimit)?;
    }
    if negative {
        value = -value;
    }
    i64::try_from(value).map_err(|_| PdfReadError::NumericLimit)
}

fn parse_decimal(bytes: &[u8]) -> Result<(LayoutUnit, SceneDecimal), PdfReadError> {
    if bytes.is_empty() || bytes.len() > 32 {
        return Err(PdfReadError::NumericLimit);
    }
    let mut index = 0;
    let negative = match bytes[0] {
        b'-' => {
            index = 1;
            true
        }
        b'+' => {
            index = 1;
            false
        }
        _ => false,
    };
    let mut whole = 0_i128;
    let mut fraction = 0_i128;
    let mut denominator = 1_i128;
    let mut after_decimal = false;
    let mut digits = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'.' && !after_decimal {
            after_decimal = true;
            index += 1;
            continue;
        }
        if !byte.is_ascii_digit() {
            return Err(PdfReadError::NumericLimit);
        }
        digits += 1;
        let digit = i128::from(byte - b'0');
        if after_decimal {
            fraction = fraction
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit))
                .ok_or(PdfReadError::NumericLimit)?;
            denominator = denominator
                .checked_mul(10)
                .ok_or(PdfReadError::NumericLimit)?;
        } else {
            whole = whole
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit))
                .ok_or(PdfReadError::NumericLimit)?;
        }
        index += 1;
    }
    if digits == 0 {
        return Err(PdfReadError::NumericLimit);
    }
    let numerator = whole
        .checked_mul(denominator)
        .and_then(|value| value.checked_add(fraction))
        .and_then(|value| value.checked_mul(i128::from(LayoutUnit::UNITS_PER_POINT)))
        .ok_or(PdfReadError::NumericLimit)?;
    let rounded = (numerator.unsigned_abs() + denominator as u128 / 2) / denominator as u128;
    let rounded = i128::try_from(rounded).map_err(|_| PdfReadError::NumericLimit)?;
    let value = if negative { -rounded } else { rounded };
    let rounded = i64::try_from(value).map_err(|_| PdfReadError::NumericLimit)?;
    let mut scale = denominator.ilog10();
    let mut coefficient = numerator / i128::from(LayoutUnit::UNITS_PER_POINT);
    if negative {
        coefficient = -coefficient;
    }
    while scale > 0 && coefficient % 10 == 0 {
        scale -= 1;
        coefficient /= 10;
    }
    let precise = match i64::try_from(coefficient) {
        Ok(coefficient) => SceneDecimal::Exact { coefficient, scale },
        Err(_) => SceneDecimal::Approximate(coefficient as f64 / 10_i128.pow(scale) as f64),
    };
    Ok((LayoutUnit::from_raw(rounded), precise))
}

#[cfg(test)]
mod tests {
    use super::{
        LayoutUnit, PaintBounds, PaintMatrix, RawPathCommand, SceneDecimal,
        lost_visible_fill_contour, separated_content_streams, transform_path,
    };

    #[test]
    fn path_bounds_preserves_empty_and_numeric_limit_results() {
        use super::{LayoutUnit, PdfPathCommand, PdfReadError, path_bounds};

        assert_eq!(
            path_bounds(&[PdfPathCommand::ClosePath]).unwrap().width,
            LayoutUnit::from_raw(0)
        );
        let zero = LayoutUnit::from_raw(0);
        let commands = [
            PdfPathCommand::MoveTo {
                x: LayoutUnit::from_raw(i64::MIN),
                y: zero,
            },
            PdfPathCommand::LineTo {
                x: LayoutUnit::from_raw(i64::MAX),
                y: zero,
            },
        ];
        assert_eq!(path_bounds(&commands), Err(PdfReadError::NumericLimit));
    }

    #[test]
    fn precise_clip_can_exclude_only_the_collapsed_contour() {
        let point = |x, y| {
            [
                SceneDecimal::Exact {
                    coefficient: x,
                    scale: 3,
                },
                SceneDecimal::Exact {
                    coefficient: y,
                    scale: 3,
                },
            ]
        };
        let fixed = |v: i64| LayoutUnit::from_raw(v * 64);
        let source = [
            RawPathCommand::MoveTo(fixed(10), fixed(10), point(10000, 10000)),
            RawPathCommand::LineTo(fixed(30), fixed(10), point(30000, 10000)),
            RawPathCommand::LineTo(fixed(30), fixed(30), point(30000, 30000)),
            RawPathCommand::Close,
            RawPathCommand::MoveTo(fixed(50), fixed(20), point(50000, 20000)),
            RawPathCommand::LineTo(fixed(50), fixed(25), point(50004, 25000)),
            RawPathCommand::LineTo(fixed(50), fixed(30), point(50000, 30000)),
        ];
        let commands = transform_path(
            &source,
            super::Matrix::IDENTITY,
            false,
            super::SceneBudget {
                remaining_scene_bytes: usize::MAX,
                remaining_image_hex_bytes: 0,
            },
        )
        .unwrap();
        let matrix = Some(PaintMatrix([1., 0., 0., 1., 0., 0.]));
        assert!(lost_visible_fill_contour(
            &source,
            &commands,
            matrix,
            PaintBounds([0., 0., 60., 40.])
        ));
        assert!(!lost_visible_fill_contour(
            &source,
            &commands,
            matrix,
            PaintBounds([0., 0., 40., 40.])
        ));
    }

    #[test]
    fn page_stream_boundary_proof_accepts_line_endings_on_either_side() {
        assert!(separated_content_streams(b"0 g\r", b"10 10 5 5 re f"));
        assert!(separated_content_streams(b"0 g", b"\n10 10 5 5 re f"));
        assert!(separated_content_streams(b"", b"0 g"));
    }

    #[test]
    fn page_stream_boundary_proof_requires_a_line_ending_after_nonempty_content() {
        assert!(!separated_content_streams(b"0 g", b"10 10 5 5 re f"));
        assert!(!separated_content_streams(
            b"0 g % note",
            b" 10 10 5 5 re f"
        ));
        assert!(!separated_content_streams(b"0 g", b""));
    }
}
