//! Read-only scene projection for the controlled PDF subset.
//!
//! The interpreter consumes the lazy reader and produces fixed-point, typed
//! elements.  It intentionally does not produce FlowDocument nodes and never
//! invokes a PDF action, URL, file, or external resource.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::layout::{LayoutRect, LayoutUnit};

use super::reader::{
    PdfObjectRef, PdfPageRecord, PdfReadDiagnostic, PdfReadDiagnosticCode, PdfReadError,
    PdfReadReport, PdfReader, PdfReaderLimits, PdfStream, PdfValue, report_active_content,
};

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

#[derive(Debug, Clone)]
struct GraphicsState {
    ctm: Matrix,
    text_matrix: Matrix,
    line_matrix: Matrix,
    font: Option<FontMap>,
    font_name: Option<String>,
    font_size: LayoutUnit,
    leading: LayoutUnit,
    char_spacing: LayoutUnit,
    word_spacing: LayoutUnit,
    horizontal_scale: LayoutUnit,
    rise: LayoutUnit,
    in_text: bool,
    path: Vec<RawPathCommand>,
    current_point: Option<(LayoutUnit, LayoutUnit)>,
    pending_clip: Option<(Vec<RawPathCommand>, bool, PdfSourceProvenance)>,
}

impl Default for GraphicsState {
    fn default() -> Self {
        Self {
            ctm: Matrix::IDENTITY,
            text_matrix: Matrix::IDENTITY,
            line_matrix: Matrix::IDENTITY,
            font: None,
            font_name: None,
            font_size: LayoutUnit::from_raw(10 * 64),
            leading: LayoutUnit::from_raw(12 * 64),
            char_spacing: LayoutUnit::from_raw(0),
            word_spacing: LayoutUnit::from_raw(0),
            horizontal_scale: Matrix::IDENTITY.a,
            rise: LayoutUnit::from_raw(0),
            in_text: false,
            path: Vec::new(),
            current_point: None,
            pending_clip: None,
        }
    }
}

#[derive(Debug, Clone)]
enum RawPathCommand {
    MoveTo(LayoutUnit, LayoutUnit),
    LineTo(LayoutUnit, LayoutUnit),
    CurveTo(
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
        LayoutUnit,
    ),
    Close,
}

#[derive(Debug, Clone)]
struct FontMap {
    name: String,
    to_unicode: BTreeMap<Vec<u8>, String>,
    simple_encoding: bool,
    widths: BTreeMap<u8, LayoutUnit>,
    default_width: LayoutUnit,
}

#[derive(Debug, Clone)]
enum ContentToken {
    Number(LayoutUnit),
    Integer(i64),
    Name(String),
    String(Vec<u8>),
    Array(Vec<ContentToken>),
    Word(String),
}

#[derive(Debug)]
struct SceneBuilder<'a> {
    reader: &'a PdfReader,
    limits: &'a PdfReaderLimits,
    page_index: u32,
    page: &'a PdfPageRecord,
    elements: Vec<PdfSceneElement>,
    report: PdfReadReport,
    fonts: BTreeMap<String, FontMap>,
    page_refs: BTreeMap<PdfObjectRef, u32>,
    state: GraphicsState,
    state_stack: Vec<GraphicsState>,
    form_depth: usize,
}

impl<'a> SceneBuilder<'a> {
    fn new(
        reader: &'a PdfReader,
        pages: &'a [PdfPageRecord],
        page_index: u32,
        page: &'a PdfPageRecord,
    ) -> Self {
        Self {
            reader,
            limits: reader.limits(),
            page_index,
            page,
            elements: Vec::new(),
            report: PdfReadReport::default(),
            fonts: BTreeMap::new(),
            page_refs: pages
                .iter()
                .enumerate()
                .filter_map(|(index, page)| {
                    u32::try_from(index)
                        .ok()
                        .map(|index| (page.object_ref, index))
                })
                .collect(),
            state: GraphicsState::default(),
            state_stack: Vec::new(),
            form_depth: 0,
        }
    }

    fn run(mut self) -> Result<PdfScenePage, PdfReadError> {
        self.inspect_annotations()?;
        for content_ref in &self.page.contents {
            match self.reader.object(*content_ref) {
                Ok(PdfValue::Stream(stream)) => {
                    self.interpret_stream(&stream, *content_ref, 0)?;
                }
                Ok(_) => self.report.push(PdfReadDiagnostic::warning(
                    PdfReadDiagnosticCode::PartialPage,
                    Some(self.page_index),
                    Some(content_ref.object_number),
                )),
                Err(PdfReadError::FilterUnsupported) => {
                    self.report.push(PdfReadDiagnostic::blocked(
                        PdfReadDiagnosticCode::UnsupportedFilter,
                        Some(self.page_index),
                        Some(content_ref.object_number),
                    ))
                }
                Err(error) => return Err(error),
            }
        }
        let partial = self.report.partial;
        Ok(PdfScenePage {
            page_index: self.page_index,
            source_object: self.page.object_ref,
            bounds: self.page.media_box,
            elements: self.elements,
            report: self.report,
            partial,
        })
    }

    fn interpret_stream(
        &mut self,
        stream: &PdfStream,
        object_ref: PdfObjectRef,
        depth: usize,
    ) -> Result<(), PdfReadError> {
        if depth > self.limits.max_form_depth {
            self.report.push(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::FormDepthLimit,
                Some(self.page_index),
                Some(object_ref.object_number),
            ));
            return Ok(());
        }
        let mut cursor = ContentCursor::new(&stream.data, self.limits.clone());
        let mut operands = Vec::new();
        while let Some((token, offset)) = cursor.next()? {
            match token {
                ContentToken::Word(operator) => {
                    self.dispatch_operator(
                        &operator, &operands, object_ref, stream, offset, depth,
                    )?;
                    operands.clear();
                }
                other => {
                    operands.push(other);
                    if operands.len() > self.limits.max_array_items {
                        return Err(PdfReadError::TokenLimit);
                    }
                }
            }
        }
        Ok(())
    }

    fn dispatch_operator(
        &mut self,
        operator: &str,
        operands: &[ContentToken],
        object_ref: PdfObjectRef,
        stream: &PdfStream,
        offset: usize,
        depth: usize,
    ) -> Result<(), PdfReadError> {
        let provenance = PdfSourceProvenance {
            object_ref,
            stream_offset: u32::try_from(
                stream
                    .source_offset
                    .checked_add(offset)
                    .ok_or(PdfReadError::NumericLimit)?,
            )
            .map_err(|_| PdfReadError::NumericLimit)?,
            operator: Some(operator.to_owned()),
        };
        match operator {
            "q" => self.state_stack.push(self.state.clone()),
            "Q" => {
                self.state = self.state_stack.pop().ok_or(PdfReadError::Syntax)?;
            }
            "cm" => {
                if let Some(matrix) = six_numbers(operands) {
                    self.state.ctm = self.state.ctm.multiply(matrix)?;
                } else {
                    return Err(PdfReadError::Syntax);
                }
            }
            "m" => {
                let (x, y) = two_numbers(operands)?;
                self.state.path.push(RawPathCommand::MoveTo(x, y));
                self.state.current_point = Some((x, y));
            }
            "l" => {
                let (x, y) = two_numbers(operands)?;
                self.state.path.push(RawPathCommand::LineTo(x, y));
                self.state.current_point = Some((x, y));
            }
            "c" => {
                let values = six_number_values(operands)?;
                self.state.path.push(RawPathCommand::CurveTo(
                    values[0], values[1], values[2], values[3], values[4], values[5],
                ));
                self.state.current_point = Some((values[4], values[5]));
            }
            "v" => {
                let values = four_number_values(operands)?;
                let (x0, y0) = self.state.current_point.ok_or(PdfReadError::Syntax)?;
                self.state.path.push(RawPathCommand::CurveTo(
                    x0, y0, values[0], values[1], values[2], values[3],
                ));
                self.state.current_point = Some((values[2], values[3]));
            }
            "y" => {
                let values = four_number_values(operands)?;
                self.state.path.push(RawPathCommand::CurveTo(
                    values[0], values[1], values[2], values[3], values[2], values[3],
                ));
                self.state.current_point = Some((values[2], values[3]));
            }
            "re" => {
                let values = four_number_values(operands)?;
                let (x, y, width, height) = (values[0], values[1], values[2], values[3]);
                self.state.path.push(RawPathCommand::MoveTo(x, y));
                self.state
                    .path
                    .push(RawPathCommand::LineTo(x.checked_add(width)?, y));
                self.state.path.push(RawPathCommand::LineTo(
                    x.checked_add(width)?,
                    y.checked_add(height)?,
                ));
                self.state
                    .path
                    .push(RawPathCommand::LineTo(x, y.checked_add(height)?));
                self.state.path.push(RawPathCommand::Close);
                self.state.current_point = Some((x, y));
            }
            "h" => self.state.path.push(RawPathCommand::Close),
            "W" | "W*" => {
                if !self.state.path.is_empty() {
                    self.state.pending_clip =
                        Some((self.state.path.clone(), operator == "W*", provenance));
                }
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "b" | "B*" | "n" => {
                self.finish_path(operator, provenance)?;
            }
            "BT" => {
                self.state.in_text = true;
                self.state.text_matrix = Matrix::IDENTITY;
                self.state.line_matrix = Matrix::IDENTITY;
            }
            "ET" => self.state.in_text = false,
            "Tf" => self.set_font(operands)?,
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
            "T*" => {
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
            }
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
                    self.dispatch_operator("T*", &[], object_ref, stream, offset, depth)?;
                }
                if let Some(ContentToken::String(bytes)) = operands.last() {
                    self.show_text(bytes, provenance)?;
                }
            }
            "TJ" => {
                if let Some(ContentToken::Array(values)) = operands.last() {
                    for value in values {
                        match value {
                            ContentToken::String(bytes) => {
                                self.show_text(bytes, provenance.clone())?
                            }
                            ContentToken::Integer(value) => {
                                let adjustment = LayoutUnit::from_raw(
                                    i64::try_from(*value)
                                        .map_err(|_| PdfReadError::NumericLimit)?
                                        .checked_mul(-1)
                                        .ok_or(PdfReadError::NumericLimit)?,
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
            "Do" => self.paint_xobject(operands, object_ref, provenance, depth)?,
            "BI" | "ID" | "EI" => self.report.push(
                PdfReadDiagnostic::blocked(
                    PdfReadDiagnosticCode::UnsupportedOperator,
                    Some(self.page_index),
                    Some(object_ref.object_number),
                )
                .with_operator(operator),
            ),
            known if is_known_noop(known) => {}
            _ => self.report.push(
                PdfReadDiagnostic::warning(
                    PdfReadDiagnosticCode::UnsupportedOperator,
                    Some(self.page_index),
                    Some(object_ref.object_number),
                )
                .with_operator(operator),
            ),
        }
        Ok(())
    }

    fn finish_path(
        &mut self,
        operator: &str,
        provenance: PdfSourceProvenance,
    ) -> Result<(), PdfReadError> {
        if let Some((commands, even_odd, clip_provenance)) = self.state.pending_clip.take() {
            let commands = transform_path(&commands, self.state.ctm)?;
            let rect = path_bounds(&commands)?;
            self.push_element(PdfSceneElement::Clip {
                value: PdfClipElement {
                    commands,
                    even_odd,
                    rect,
                    provenance: clip_provenance,
                },
            })?;
        }
        if !self.state.path.is_empty() && operator != "n" {
            let commands = transform_path(&self.state.path, self.state.ctm)?;
            let rect = path_bounds(&commands)?;
            self.push_element(PdfSceneElement::Path {
                value: PdfPathElement {
                    commands,
                    rect,
                    stroke: matches!(operator, "S" | "s" | "B" | "b" | "B*"),
                    fill: matches!(operator, "f" | "F" | "f*" | "B" | "b" | "B*"),
                    provenance,
                },
            })?;
        }
        self.state.path.clear();
        self.state.current_point = None;
        Ok(())
    }

    fn set_font(&mut self, operands: &[ContentToken]) -> Result<(), PdfReadError> {
        let Some(ContentToken::Name(name)) = operands.first() else {
            return Err(PdfReadError::Syntax);
        };
        let size = operands
            .get(1)
            .and_then(ContentToken::number)
            .ok_or(PdfReadError::Syntax)?;
        let font = if let Some(font) = self.fonts.get(name) {
            font.clone()
        } else {
            let font = self.load_font(name)?;
            self.fonts.insert(name.clone(), font.clone());
            font
        };
        self.state.font_name = Some(name.clone());
        self.state.font = Some(font);
        self.state.font_size = size;
        Ok(())
    }

    fn load_font(&self, name: &str) -> Result<FontMap, PdfReadError> {
        let resources = self
            .page
            .resources
            .as_ref()
            .ok_or(PdfReadError::ReferenceInvalid)?
            .as_dictionary()
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let fonts = resources
            .get("Font")
            .ok_or(PdfReadError::ReferenceInvalid)?
            .as_dictionary()
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let font_value = fonts.get(name).ok_or(PdfReadError::ReferenceInvalid)?;
        let font_ref = font_value
            .as_reference()
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let font = self.reader.object(font_ref)?;
        let dictionary = font.as_dictionary().ok_or(PdfReadError::ReferenceInvalid)?;
        let base_name = dictionary
            .get("BaseFont")
            .and_then(PdfValue::as_name)
            .unwrap_or(name)
            .to_owned();
        let mut to_unicode = BTreeMap::new();
        if let Some(to_unicode_value) = dictionary.get("ToUnicode") {
            let to_unicode_ref = to_unicode_value
                .as_reference()
                .ok_or(PdfReadError::ReferenceInvalid)?;
            let stream = self.reader.object(to_unicode_ref)?;
            if let PdfValue::Stream(stream) = stream {
                to_unicode = parse_to_unicode(&stream.data, self.limits)?;
            }
        }
        let simple_encoding = dictionary
            .get("Subtype")
            .and_then(PdfValue::as_name)
            .is_none_or(|value| value != "Type0");
        let default_width = dictionary
            .get("MissingWidth")
            .and_then(font_width)
            .unwrap_or(LayoutUnit::from_raw(500));
        let widths = dictionary
            .get("Widths")
            .and_then(PdfValue::as_array)
            .map(|values| {
                let first = dictionary
                    .get("FirstChar")
                    .and_then(PdfValue::as_integer)
                    .unwrap_or(0);
                values
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| {
                        let code = first.checked_add(i64::try_from(index).ok()?)?;
                        let code = u8::try_from(code).ok()?;
                        Some((code, font_width(value).unwrap_or(default_width)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !simple_encoding && to_unicode.is_empty() {
            // The scene is still usable as a visual projection, but the
            // diagnostic is added at the first shown glyph in show_text.
        }
        Ok(FontMap {
            name: base_name,
            to_unicode,
            simple_encoding,
            widths,
            default_width,
        })
    }

    fn show_text(
        &mut self,
        bytes: &[u8],
        provenance: PdfSourceProvenance,
    ) -> Result<(), PdfReadError> {
        if !self.state.in_text {
            return Err(PdfReadError::Syntax);
        }
        let font = self
            .state
            .font
            .clone()
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let mut glyphs = Vec::new();
        let mut text = String::new();
        let mut mapping = if font.to_unicode.is_empty() {
            if font.simple_encoding {
                PdfTextMapping::SimpleEncoding
            } else {
                PdfTextMapping::Missing
            }
        } else {
            PdfTextMapping::ToUnicode
        };
        let mut index = 0;
        while index < bytes.len() {
            let (code_bytes, code) = next_code(bytes, index, &font.to_unicode);
            index += code_bytes.len();
            let unicode = if let Some(mapped) = font.to_unicode.get(&code_bytes) {
                Some(mapped.clone())
            } else if font.simple_encoding && code <= 0x7F {
                char::from_u32(u32::from(code)).map(|value| value.to_string())
            } else {
                mapping = PdfTextMapping::Missing;
                None
            };
            if let Some(unicode) = &unicode {
                text.push_str(unicode);
            } else {
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
            let (x, y) = self
                .state
                .ctm
                .multiply(self.state.text_matrix)?
                .point(LayoutUnit::from_raw(0), self.state.rise)?;
            let rect = LayoutRect {
                x,
                y: y.checked_sub(self.state.font_size)?,
                width: advance,
                height: self.state.font_size,
            };
            glyphs.push(PdfGlyph {
                code: u32::from(code),
                unicode,
                rect,
                advance,
                provenance: provenance.clone(),
            });
            self.advance_text(advance)?;
            if glyphs.len() > self.limits.max_scene_elements {
                return Err(PdfReadError::TokenLimit);
            }
        }
        if let Some(first) = glyphs.first() {
            let rect = union_rect(glyphs.iter().map(|glyph| glyph.rect))?;
            let _ = first;
            self.push_element(PdfSceneElement::Text {
                value: PdfTextElement {
                    text,
                    rect,
                    font_name: Some(font.name),
                    mapping,
                    glyphs,
                    provenance,
                },
            })?;
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
        object_ref: PdfObjectRef,
        provenance: PdfSourceProvenance,
        depth: usize,
    ) -> Result<(), PdfReadError> {
        let Some(ContentToken::Name(name)) = operands.last() else {
            return Err(PdfReadError::Syntax);
        };
        let resources = self
            .page
            .resources
            .as_ref()
            .and_then(PdfValue::as_dictionary)
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let xobjects = resources
            .get("XObject")
            .and_then(PdfValue::as_dictionary)
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let xobject_ref = xobjects
            .get(name)
            .and_then(PdfValue::as_reference)
            .ok_or(PdfReadError::ReferenceInvalid)?;
        let xobject = self.reader.object(xobject_ref)?;
        let stream = match xobject {
            PdfValue::Stream(stream) => stream,
            _ => return Err(PdfReadError::ReferenceInvalid),
        };
        let dictionary = &stream.dict;
        match dictionary.get("Subtype").and_then(PdfValue::as_name) {
            Some("Image") => self.paint_image(&stream, xobject_ref, provenance),
            Some("Form") => {
                if depth + 1 > self.limits.max_form_depth {
                    self.report.push(PdfReadDiagnostic::blocked(
                        PdfReadDiagnosticCode::FormDepthLimit,
                        Some(self.page_index),
                        Some(xobject_ref.object_number),
                    ));
                    return Ok(());
                }
                let form_bounds = LayoutRect {
                    x: LayoutUnit::from_raw(0),
                    y: LayoutUnit::from_raw(0),
                    width: dictionary
                        .get("BBox")
                        .and_then(|value| parse_bbox(value).ok())
                        .map(|rect| rect.width)
                        .unwrap_or(LayoutUnit::from_raw(0)),
                    height: dictionary
                        .get("BBox")
                        .and_then(|value| parse_bbox(value).ok())
                        .map(|rect| rect.height)
                        .unwrap_or(LayoutUnit::from_raw(0)),
                };
                self.push_element(PdfSceneElement::Annotation {
                    value: PdfAnnotationElement {
                        subtype: Some("FormXObject".to_owned()),
                        kind: PdfAnnotationKind::Supported,
                        rect: form_bounds,
                        provenance: PdfSourceProvenance {
                            object_ref: xobject_ref,
                            ..provenance.clone()
                        },
                    },
                })?;
                self.form_depth += 1;
                self.interpret_stream(&stream, xobject_ref, depth + 1)?;
                self.form_depth = self.form_depth.saturating_sub(1);
                Ok(())
            }
            _ => {
                self.report.push(
                    PdfReadDiagnostic::warning(
                        PdfReadDiagnosticCode::UnsupportedOperator,
                        Some(self.page_index),
                        Some(object_ref.object_number),
                    )
                    .with_operator("Do"),
                );
                Ok(())
            }
        }
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
            .and_then(PdfValue::as_name)
            .unwrap_or("Identity")
            .to_owned();
        let media_type = if filter == "DCTDecode" || filter == "DCT" {
            "image/jpeg"
        } else {
            "application/octet-stream"
        };
        if stream.data.len() > self.limits.max_stream_bytes / 2 {
            self.report.push(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::ImageLimit,
                Some(self.page_index),
                Some(object_ref.object_number),
            ));
            return Ok(());
        }
        let matrix = self.state.ctm;
        let (x, y) = matrix.point(LayoutUnit::from_raw(0), LayoutUnit::from_raw(0))?;
        let (x1, y1) = matrix.point(
            LayoutUnit::from_raw(i64::from(width)),
            LayoutUnit::from_raw(i64::from(height)),
        )?;
        let rect = LayoutRect {
            x,
            y,
            width: x1.checked_sub(x)?,
            height: y1.checked_sub(y)?,
        };
        self.push_element(PdfSceneElement::Image {
            value: PdfImageElement {
                width,
                height,
                media_type: media_type.to_owned(),
                filter,
                data_hex: hex_encode(&stream.data),
                rect,
                provenance: PdfSourceProvenance {
                    object_ref,
                    ..provenance
                },
            },
        })?;
        Ok(())
    }

    fn inspect_annotations(&mut self) -> Result<(), PdfReadError> {
        for annotation_ref in &self.page.annotations {
            let value = self.reader.object(*annotation_ref)?;
            let dictionary = value
                .as_dictionary()
                .ok_or(PdfReadError::ReferenceInvalid)?;
            let subtype = dictionary
                .get("Subtype")
                .and_then(PdfValue::as_name)
                .map(str::to_owned);
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
            };
            if subtype.as_deref() == Some("Link") {
                if let Some(action) = dictionary.get("A").and_then(PdfValue::as_dictionary) {
                    let action_type = action.get("S").and_then(PdfValue::as_name).unwrap_or("");
                    if action_type != "GoTo" {
                        self.report.push(PdfReadDiagnostic::blocked(
                            PdfReadDiagnosticCode::ActiveContent,
                            Some(self.page_index),
                            Some(annotation_ref.object_number),
                        ));
                    }
                }
                let destination = dictionary
                    .get("Dest")
                    .or_else(|| {
                        dictionary
                            .get("A")
                            .and_then(PdfValue::as_dictionary)
                            .and_then(|value| value.get("D"))
                    })
                    .and_then(PdfValue::as_array)
                    .and_then(|value| value.first())
                    .and_then(PdfValue::as_reference)
                    .and_then(|reference| self.page_refs.get(&reference).copied());
                self.push_element(PdfSceneElement::Link {
                    value: PdfLinkElement {
                        destination_page: destination,
                        rect,
                        internal: destination.is_some(),
                        provenance,
                    },
                })?;
            } else if subtype.as_deref() == Some("Widget") {
                let field_type = dictionary
                    .get("FT")
                    .and_then(PdfValue::as_name)
                    .map(str::to_owned);
                let name = string_value(dictionary.get("T"));
                let value = string_value(dictionary.get("V"));
                self.push_element(PdfSceneElement::Form {
                    value: PdfFormElement {
                        field_type,
                        name,
                        value,
                        rect,
                        provenance,
                    },
                })?;
            } else {
                self.report.push(PdfReadDiagnostic::warning(
                    PdfReadDiagnosticCode::UnsupportedAnnotation,
                    Some(self.page_index),
                    Some(annotation_ref.object_number),
                ));
                self.push_element(PdfSceneElement::Annotation {
                    value: PdfAnnotationElement {
                        subtype,
                        kind: PdfAnnotationKind::Unsupported,
                        rect,
                        provenance,
                    },
                })?;
            }
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
        }
        Ok(())
    }

    fn push_element(&mut self, element: PdfSceneElement) -> Result<(), PdfReadError> {
        if self.elements.len() >= self.limits.max_scene_elements {
            self.report.push(PdfReadDiagnostic::blocked(
                PdfReadDiagnosticCode::SceneLimit,
                Some(self.page_index),
                None,
            ));
            return Err(PdfReadError::TokenLimit);
        }
        self.elements.push(element);
        Ok(())
    }
}

/// Reads a bounded page window and calculates a stable scene hash.
pub fn read_pdf_scene(
    reader: &PdfReader,
    request: &PdfSceneRequest,
) -> Result<PdfScene, PdfReadError> {
    let pages = reader.page_records()?;
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
    let mut report = reader.preflight_report();
    if let Ok(catalog) = reader.object(reader.root_for_scene())
        && let Some(dictionary) = catalog.as_dictionary()
    {
        report_active_content(
            dictionary,
            None,
            Some(reader.root_for_scene().object_number),
            &mut report,
        );
    }
    let mut scene_pages = Vec::with_capacity(end.saturating_sub(first));
    for (index, page) in pages.iter().enumerate().take(end).skip(first) {
        let page_index = u32::try_from(index).map_err(|_| PdfReadError::PageCountLimit)?;
        match SceneBuilder::new(reader, &pages, page_index, page).run() {
            Ok(page) => {
                report.extend(&page.report);
                scene_pages.push(page);
            }
            Err(PdfReadError::FilterUnsupported) => {
                report.push(PdfReadDiagnostic::blocked(
                    PdfReadDiagnosticCode::UnsupportedFilter,
                    Some(page_index),
                    None,
                ));
            }
            Err(error) => return Err(error),
        }
    }
    let mut scene = PdfScene {
        schema_version: PDF_SCENE_SCHEMA_VERSION,
        source_hash: reader.source_hash().to_owned(),
        page_count,
        first_page: request.first_page,
        pages: scene_pages,
        report,
        result_hash: String::new(),
    };
    let bytes = serde_json::to_vec(&scene).map_err(|_| PdfReadError::Syntax)?;
    scene.result_hash = blake3::hash(&bytes).to_hex().to_string();
    Ok(scene)
}

fn is_known_noop(operator: &str) -> bool {
    matches!(
        operator,
        "w" | "J"
            | "j"
            | "M"
            | "d"
            | "ri"
            | "i"
            | "gs"
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
    fn number(&self) -> Option<LayoutUnit> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Integer(value) => i64::try_from(*value)
                .ok()
                .and_then(|value| value.checked_mul(LayoutUnit::UNITS_PER_POINT))
                .map(LayoutUnit::from_raw),
            _ => None,
        }
    }
}

fn font_width(value: &PdfValue) -> Option<LayoutUnit> {
    match value {
        PdfValue::Integer(value) => i64::try_from(*value).ok().map(LayoutUnit::from_raw),
        PdfValue::Real(value) => Some(*value),
        _ => None,
    }
}

fn one_number(values: &[ContentToken]) -> Result<LayoutUnit, PdfReadError> {
    values
        .last()
        .and_then(ContentToken::number)
        .ok_or(PdfReadError::Syntax)
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
) -> Result<Vec<PdfPathCommand>, PdfReadError> {
    commands
        .iter()
        .map(|command| match command {
            RawPathCommand::MoveTo(x, y) => {
                let (x, y) = matrix.point(*x, *y)?;
                Ok(PdfPathCommand::MoveTo { x, y })
            }
            RawPathCommand::LineTo(x, y) => {
                let (x, y) = matrix.point(*x, *y)?;
                Ok(PdfPathCommand::LineTo { x, y })
            }
            RawPathCommand::CurveTo(x1, y1, x2, y2, x3, y3) => {
                let (x1, y1) = matrix.point(*x1, *y1)?;
                let (x2, y2) = matrix.point(*x2, *y2)?;
                let (x3, y3) = matrix.point(*x3, *y3)?;
                Ok(PdfPathCommand::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x3,
                    y3,
                })
            }
            RawPathCommand::Close => Ok(PdfPathCommand::ClosePath),
        })
        .collect()
}

fn path_bounds(commands: &[PdfPathCommand]) -> Result<LayoutRect, PdfReadError> {
    let mut points = Vec::new();
    for command in commands {
        match command {
            PdfPathCommand::MoveTo { x, y } | PdfPathCommand::LineTo { x, y } => {
                points.push((*x, *y))
            }
            PdfPathCommand::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x3,
                y3,
            } => points.extend([(*x1, *y1), (*x2, *y2), (*x3, *y3)]),
            PdfPathCommand::ClosePath => {}
        }
    }
    if points.is_empty() {
        return Ok(LayoutRect {
            x: LayoutUnit::from_raw(0),
            y: LayoutUnit::from_raw(0),
            width: LayoutUnit::from_raw(0),
            height: LayoutUnit::from_raw(0),
        });
    }
    let min_x = points
        .iter()
        .map(|(x, _)| x.raw())
        .min()
        .ok_or(PdfReadError::NumericLimit)?;
    let min_y = points
        .iter()
        .map(|(_, y)| y.raw())
        .min()
        .ok_or(PdfReadError::NumericLimit)?;
    let max_x = points
        .iter()
        .map(|(x, _)| x.raw())
        .max()
        .ok_or(PdfReadError::NumericLimit)?;
    let max_y = points
        .iter()
        .map(|(_, y)| y.raw())
        .max()
        .ok_or(PdfReadError::NumericLimit)?;
    Ok(LayoutRect {
        x: LayoutUnit::from_raw(min_x),
        y: LayoutUnit::from_raw(min_y),
        width: LayoutUnit::from_raw(max_x.checked_sub(min_x).ok_or(PdfReadError::NumericLimit)?),
        height: LayoutUnit::from_raw(max_y.checked_sub(min_y).ok_or(PdfReadError::NumericLimit)?),
    })
}

fn union_rect(rects: impl Iterator<Item = LayoutRect>) -> Result<LayoutRect, PdfReadError> {
    let rects = rects.collect::<Vec<_>>();
    if rects.is_empty() {
        return Err(PdfReadError::NumericLimit);
    }
    let min_x = rects
        .iter()
        .map(|rect| rect.x.raw())
        .min()
        .ok_or(PdfReadError::NumericLimit)?;
    let min_y = rects
        .iter()
        .map(|rect| rect.y.raw())
        .min()
        .ok_or(PdfReadError::NumericLimit)?;
    let max_x = rects
        .iter()
        .map(|rect| rect.x.raw().checked_add(rect.width.raw()))
        .collect::<Option<Vec<_>>>()
        .and_then(|values| values.into_iter().max())
        .ok_or(PdfReadError::NumericLimit)?;
    let max_y = rects
        .iter()
        .map(|rect| rect.y.raw().checked_add(rect.height.raw()))
        .collect::<Option<Vec<_>>>()
        .and_then(|values| values.into_iter().max())
        .ok_or(PdfReadError::NumericLimit)?;
    Ok(LayoutRect {
        x: LayoutUnit::from_raw(min_x),
        y: LayoutUnit::from_raw(min_y),
        width: LayoutUnit::from_raw(max_x.checked_sub(min_x).ok_or(PdfReadError::NumericLimit)?),
        height: LayoutUnit::from_raw(max_y.checked_sub(min_y).ok_or(PdfReadError::NumericLimit)?),
    })
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

fn string_value(value: Option<&PdfValue>) -> Option<String> {
    value
        .and_then(PdfValue::as_bytes)
        .and_then(|value| String::from_utf8(value.to_vec()).ok())
}

fn next_code(bytes: &[u8], index: usize, mapping: &BTreeMap<Vec<u8>, String>) -> (Vec<u8>, u8) {
    let remaining = &bytes[index..];
    let mut best = None;
    for key in mapping.keys() {
        if key.len() > remaining.len() || !remaining.starts_with(key) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|value: &Vec<u8>| key.len() > value.len())
        {
            best = Some(key.clone());
        }
    }
    let code_bytes = best.unwrap_or_else(|| vec![remaining[0]]);
    let code = code_bytes
        .iter()
        .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte));
    (code_bytes, u8::try_from(code).unwrap_or(0))
}

fn parse_to_unicode(
    bytes: &[u8],
    limits: &PdfReaderLimits,
) -> Result<BTreeMap<Vec<u8>, String>, PdfReadError> {
    let text = String::from_utf8_lossy(bytes);
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let mut map = BTreeMap::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == "beginbfchar" {
            index += 1;
            while index < tokens.len() && tokens[index] != "endbfchar" {
                let source = parse_hex_token(tokens.get(index).ok_or(PdfReadError::Syntax)?)?;
                let destination =
                    parse_utf16_hex(tokens.get(index + 1).ok_or(PdfReadError::Syntax)?)?;
                map.insert(source, destination);
                index += 2;
                if map.len() > limits.max_array_items {
                    return Err(PdfReadError::ObjectLimit);
                }
            }
        } else if tokens[index] == "beginbfrange" {
            index += 1;
            while index < tokens.len() && tokens[index] != "endbfrange" {
                let low = parse_hex_token(tokens.get(index).ok_or(PdfReadError::Syntax)?)?;
                let high = parse_hex_token(tokens.get(index + 1).ok_or(PdfReadError::Syntax)?)?;
                let destination = *tokens.get(index + 2).ok_or(PdfReadError::Syntax)?;
                let low_value = bytes_to_u32(&low);
                let high_value = bytes_to_u32(&high);
                if destination.starts_with('[') {
                    index += 3;
                    continue;
                }
                let destination_bytes = parse_hex_token(destination)?;
                let destination_start = bytes_to_u32(&destination_bytes);
                for value in low_value..=high_value {
                    let source = u32_to_bytes(value, low.len());
                    let unicode = parse_utf16_codepoint(
                        destination_start
                            .checked_add(value - low_value)
                            .ok_or(PdfReadError::NumericLimit)?,
                    );
                    map.insert(source, unicode);
                    if map.len() > limits.max_array_items {
                        return Err(PdfReadError::ObjectLimit);
                    }
                }
                index += 3;
            }
        }
        index += 1;
    }
    Ok(map)
}

fn parse_hex_token(token: &str) -> Result<Vec<u8>, PdfReadError> {
    let bytes = token.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'<' || *bytes.last().ok_or(PdfReadError::Syntax)? != b'>' {
        return Err(PdfReadError::Syntax);
    }
    let mut result = Vec::new();
    let mut high = None;
    for byte in &bytes[1..bytes.len() - 1] {
        let Some(nibble) = hex_nibble(*byte) else {
            return Err(PdfReadError::Syntax);
        };
        if let Some(high) = high.take() {
            result.push((high << 4) | nibble);
        } else {
            high = Some(nibble);
        }
    }
    if let Some(high) = high {
        result.push(high << 4);
    }
    Ok(result)
}

fn parse_utf16_hex(token: &str) -> Result<String, PdfReadError> {
    let bytes = parse_hex_token(token)?;
    if bytes.len() % 2 != 0 {
        return Err(PdfReadError::Syntax);
    }
    let units = bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| PdfReadError::Syntax)
}

fn bytes_to_u32(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
}

fn u32_to_bytes(value: u32, width: usize) -> Vec<u8> {
    (0..width)
        .rev()
        .map(|shift| u8::try_from((value >> (shift * 8)) & 0xFF).unwrap_or(0))
        .collect()
}

fn parse_utf16_codepoint(value: u32) -> String {
    char::from_u32(value).unwrap_or('\u{FFFD}').to_string()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0F)] as char);
    }
    output
}

struct ContentCursor<'a> {
    bytes: &'a [u8],
    position: usize,
    limits: PdfReaderLimits,
    tokens: usize,
}

impl<'a> ContentCursor<'a> {
    fn new(bytes: &'a [u8], limits: PdfReaderLimits) -> Self {
        Self {
            bytes,
            position: 0,
            limits,
            tokens: 0,
        }
    }

    fn next(&mut self) -> Result<Option<(ContentToken, usize)>, PdfReadError> {
        self.skip_space();
        if self.position >= self.bytes.len() {
            return Ok(None);
        }
        self.tokens = self.tokens.checked_add(1).ok_or(PdfReadError::TokenLimit)?;
        if self.tokens > self.limits.max_content_tokens {
            return Err(PdfReadError::TokenLimit);
        }
        let offset = self.position;
        let token = match self.bytes[self.position] {
            b'(' => ContentToken::String(self.literal_string()?),
            b'<' => {
                if self.bytes.get(self.position + 1) == Some(&b'<') {
                    return Err(PdfReadError::Syntax);
                }
                ContentToken::String(self.hex_string()?)
            }
            b'[' => self.array()?,
            b'/' => ContentToken::Name(self.name()?),
            _ => {
                let word = self.word();
                if word
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'+' | b'-'))
                {
                    ContentToken::Integer(parse_signed_i64(&word)?)
                } else if word
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'+' | b'-' | b'.'))
                    && word.contains(&b'.')
                {
                    ContentToken::Number(parse_decimal(&word)?)
                } else {
                    ContentToken::Word(String::from_utf8(word).map_err(|_| PdfReadError::Syntax)?)
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
                .is_some_and(|byte| byte.is_ascii_whitespace())
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
        }
    }

    fn word(&mut self) -> Vec<u8> {
        let start = self.position;
        while self.position < self.bytes.len() && !is_content_delimiter(self.bytes[self.position]) {
            self.position += 1;
        }
        self.bytes[start..self.position].to_vec()
    }

    fn name(&mut self) -> Result<String, PdfReadError> {
        self.position += 1;
        let start = self.position;
        while self.position < self.bytes.len() && !is_content_delimiter(self.bytes[self.position]) {
            self.position += 1;
        }
        String::from_utf8(self.bytes[start..self.position].to_vec())
            .map_err(|_| PdfReadError::Syntax)
    }

    fn literal_string(&mut self) -> Result<Vec<u8>, PdfReadError> {
        self.position += 1;
        let mut output = Vec::new();
        let mut nesting = 1_usize;
        while self.position < self.bytes.len() {
            let byte = self.bytes[self.position];
            self.position += 1;
            match byte {
                b'(' => {
                    nesting += 1;
                    output.push(byte);
                }
                b')' => {
                    nesting = nesting.checked_sub(1).ok_or(PdfReadError::Syntax)?;
                    if nesting == 0 {
                        return Ok(output);
                    }
                    output.push(byte);
                }
                b'\\' => {
                    let escaped = self
                        .bytes
                        .get(self.position)
                        .copied()
                        .ok_or(PdfReadError::Syntax)?;
                    self.position += 1;
                    match escaped {
                        b'n' => output.push(b'\n'),
                        b'r' => output.push(b'\r'),
                        b't' => output.push(b'\t'),
                        b'b' => output.push(8),
                        b'f' => output.push(12),
                        b'\r' => {
                            if self.bytes.get(self.position) == Some(&b'\n') {
                                self.position += 1;
                            }
                        }
                        b'\n' => {}
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
                                value = value * 8 + next - b'0';
                            }
                            output.push(value);
                        }
                        other => output.push(other),
                    }
                }
                other => output.push(other),
            }
            if output.len() > self.limits.max_string_bytes {
                return Err(PdfReadError::StreamLimit);
            }
        }
        Err(PdfReadError::Syntax)
    }

    fn hex_string(&mut self) -> Result<Vec<u8>, PdfReadError> {
        let start = self.position;
        self.position += 1;
        while self.position < self.bytes.len() && self.bytes[self.position] != b'>' {
            self.position += 1;
        }
        if self.position >= self.bytes.len() {
            return Err(PdfReadError::Syntax);
        }
        self.position += 1;
        let token = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| PdfReadError::Syntax)?;
        parse_hex_token(token)
    }

    fn array(&mut self) -> Result<ContentToken, PdfReadError> {
        self.position += 1;
        let mut values = Vec::new();
        loop {
            self.skip_space();
            if self.bytes.get(self.position) == Some(&b']') {
                self.position += 1;
                return Ok(ContentToken::Array(values));
            }
            if values.len() >= self.limits.max_array_items {
                return Err(PdfReadError::TokenLimit);
            }
            let Some((value, _)) = self.next()? else {
                return Err(PdfReadError::Syntax);
            };
            values.push(value);
        }
    }
}

fn is_content_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(byte, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'/' | b'%')
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

fn parse_decimal(bytes: &[u8]) -> Result<LayoutUnit, PdfReadError> {
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
    i64::try_from(value)
        .map(LayoutUnit::from_raw)
        .map_err(|_| PdfReadError::NumericLimit)
}
