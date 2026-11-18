use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    io::Cursor,
    ops::Range,
};

use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};

use super::super::{
    reader::{PdfObjectRef, PdfReaderLimits, PdfStream, PdfValue},
    scene::{ContentCursor, ContentToken, NativePdfNotePage},
};
use super::{
    NativeBinding, NativePageId, NativeSessionError,
    graph::{NativePdfGraph, collect_refs, spend},
};

pub const NATIVE_MAX_IMAGE_BYTES: usize = 60 * 1024;
pub const NATIVE_MAX_IMAGE_PIXELS: u64 = 16_000_000;
pub const NATIVE_MAX_IMAGE_DIMENSION: u32 = crate::asset::MAX_IMAGE_DIMENSION;
pub const NATIVE_MAX_GRAPHIC_POINTS: usize = 32;
pub const NATIVE_MAX_STROKE_WIDTH: i64 = 32_000;
const MAX_OBJECTS_PER_PAGE: usize = 128;
const MAX_CONTENT_BYTES: usize = 512 * 1024;
const MAX_FORM_DEPTH: usize = 8;
const MAX_COORDINATE: i64 = 1_000_000_000;
const UNIT: f64 = 1000.0;

type Dict = BTreeMap<String, PdfValue>;
type Bounds = [i64; 4];
type Point = [f64; 2];
type Result<T> = std::result::Result<T, NativeSessionError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeObjectRefusal {
    #[serde(rename = "FLOW_PDF_NATIVE_OBJECT_STATE")]
    State,
    #[serde(rename = "FLOW_PDF_NATIVE_OBJECT_GEOMETRY")]
    Geometry,
    #[serde(rename = "FLOW_PDF_NATIVE_OBJECT_ASSET")]
    Asset,
    #[serde(rename = "FLOW_PDF_NATIVE_OBJECT_PROFILE")]
    Profile,
}

impl NativeObjectRefusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::State => "FLOW_PDF_NATIVE_OBJECT_STATE",
            Self::Geometry => "FLOW_PDF_NATIVE_OBJECT_GEOMETRY",
            Self::Asset => "FLOW_PDF_NATIVE_OBJECT_ASSET",
            Self::Profile => "FLOW_PDF_NATIVE_OBJECT_PROFILE",
        }
    }
}

use NativeObjectRefusal::{Asset, Geometry, Profile, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeObjectProfile {
    RasterRgb8,
    RasterGray8,
    Line,
    Rectangle,
    Polygon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeObjectKind {
    Image,
    Graphic,
}

impl NativeObjectProfile {
    fn kind(self) -> NativeObjectKind {
        match self {
            Self::RasterRgb8 | Self::RasterGray8 => NativeObjectKind::Image,
            _ => NativeObjectKind::Graphic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeObjectOperation {
    Replace,
    Move,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeObjectView {
    pub id: [u8; 32],
    pub kind: NativeObjectKind,
    pub profile: NativeObjectProfile,
    pub bounds: Bounds,
    pub operations: [NativeObjectOperation; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeObjectPage {
    pub page: NativePageId,
    pub can_add: bool,
    pub objects: Vec<NativeObjectView>,
    pub refusal: Option<NativeObjectRefusal>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeObjectTarget {
    pub page: NativePageId,
    pub id: [u8; 32],
    pub profile: NativeObjectProfile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeGraphicShape {
    Line,
    Rectangle,
    Polygon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeGraphic {
    pub shape: NativeGraphicShape,
    pub points: Vec<[i64; 2]>,
    pub fill: Option<[u8; 3]>,
    pub stroke: Option<[u8; 3]>,
    pub stroke_width: i64,
}

// @contract: Bounds and points use thousandths of a PDF point in unrotated page space. Affine translations use PDF points.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NativeObjectCommand {
    AddGraphic {
        page: NativePageId,
        graphic: NativeGraphic,
    },
    ReplaceGraphic {
        target: NativeObjectTarget,
        graphic: NativeGraphic,
    },
    AddImage {
        page: NativePageId,
        bounds: Bounds,
        asset_hex: String,
    },
    ReplaceImage {
        target: NativeObjectTarget,
        asset_hex: String,
    },
    MoveObject {
        target: NativeObjectTarget,
        transform: [f64; 6],
    },
    RemoveObject {
        target: NativeObjectTarget,
    },
}

impl NativeObjectCommand {
    fn target(&self) -> Option<NativeObjectTarget> {
        match self {
            Self::ReplaceGraphic { target, .. }
            | Self::ReplaceImage { target, .. }
            | Self::MoveObject { target, .. }
            | Self::RemoveObject { target } => Some(*target),
            _ => None,
        }
    }

    pub(super) fn validate_size(&self) -> Result<usize> {
        match self {
            Self::AddImage { asset_hex, .. } | Self::ReplaceImage { asset_hex, .. } => {
                if asset_hex.is_empty()
                    || asset_hex.len() > 2 * NATIVE_MAX_IMAGE_BYTES
                    || asset_hex.len() % 2 != 0
                {
                    return Err(Asset.into());
                }
                Ok(asset_hex.len() * 3)
            }
            Self::AddGraphic { graphic, .. } | Self::ReplaceGraphic { graphic, .. }
                if graphic.points.len() > NATIVE_MAX_GRAPHIC_POINTS =>
            {
                Err(Geometry.into())
            }
            _ => Ok(0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);

    fn checked(self) -> Result<Self> {
        let [a, b, c, d, _, _] = self.0;
        let diagonal = b == 0. && c == 0. && a * d > 0.;
        let quarter_turn = a == 0. && d == 0. && -b * c > 0.;
        if !(diagonal || quarter_turn)
            || self
                .0
                .iter()
                .any(|n| !n.is_finite() || n.abs() > 1_000_000.)
            || (a * d - b * c) < 1e-10
            || [a, b, c, d].into_iter().any(|n| n != 0. && n.abs() < 1e-6)
        {
            return Err(Geometry.into());
        }
        Ok(self)
    }

    fn point(self, [x, y]: Point) -> Point {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }

    fn then(self, outer: Self) -> Result<Self> {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, _, _] = outer.0;
        let [x, y] = outer.point([e, f]);
        Self([
            g * a + i * b,
            h * a + j * b,
            g * c + i * d,
            h * c + j * d,
            x,
            y,
        ])
        .checked()
    }

    fn inverse(self) -> Result<Self> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        Self([
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * f - d * e) / det,
            (b * e - a * f) / det,
        ])
        .checked()
    }

    fn rect(self, [l, b, r, t]: [f64; 4]) -> [f64; 4] {
        let [x, y] = self.point([l, b]);
        let [z, w] = self.point([r, t]);
        [x.min(z), y.min(w), x.max(z), y.max(w)]
    }

    fn operator(self) -> String {
        let [a, b, c, d, e, f] = self.0;
        format!("{a:.12} {b:.12} {c:.12} {d:.12} {e:.12} {f:.12} cm")
    }
}

fn contains(outer: [f64; 4], inner: [f64; 4]) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

fn positive([l, b, r, t]: [f64; 4]) -> bool {
    l < r
        && b < t
        && [l, b, r, t]
            .iter()
            .all(|v| v.abs() * UNIT <= MAX_COORDINATE as f64)
}

fn bounds(rect: [f64; 4]) -> Result<Bounds> {
    if !positive(rect) {
        return Err(Geometry.into());
    }
    let value = rect.map(|v| (v * UNIT).round() as i64);
    if value[0] >= value[2] || value[1] >= value[3] {
        return Err(Geometry.into());
    }
    Ok(value)
}

fn page_space(raw: Bounds) -> [f64; 4] {
    raw.map(|n| n as f64 / UNIT)
}

fn resolve<'a>(graph: &'a NativePdfGraph, mut value: &'a PdfValue) -> Option<&'a PdfValue> {
    for _ in 0..graph.limits.max_nesting {
        match value.as_reference() {
            Some(reference) => value = graph.objects.get(&reference)?,
            None => return Some(value),
        }
    }
    None
}

fn stream(graph: &NativePdfGraph, reference: PdfObjectRef) -> Result<&PdfStream> {
    match graph.objects.get(&reference) {
        Some(PdfValue::Stream(stream)) => Ok(stream),
        _ => Err(State.into()),
    }
}

fn page_dict(graph: &NativePdfGraph, page: PdfObjectRef) -> Result<&Dict> {
    graph
        .objects
        .get(&page)
        .and_then(PdfValue::as_dictionary)
        .ok_or(State.into())
}

fn resources(graph: &NativePdfGraph, mut page: PdfObjectRef) -> Result<Option<&Dict>> {
    for _ in 0..graph.limits.max_nesting {
        let dict = page_dict(graph, page)?;
        if let Some(value) = dict.get("Resources") {
            return Ok(Some(
                resolve(graph, value)
                    .and_then(PdfValue::as_dictionary)
                    .ok_or(State)?,
            ));
        }
        match dict.get("Parent").and_then(PdfValue::as_reference) {
            Some(parent) => page = parent,
            None => return Ok(None),
        }
    }
    Err(State.into())
}

fn xobjects<'a>(
    graph: &'a NativePdfGraph,
    resources: Option<&'a Dict>,
) -> Result<Option<&'a Dict>> {
    resources
        .and_then(|dict| dict.get("XObject"))
        .map(|value| {
            resolve(graph, value)
                .and_then(PdfValue::as_dictionary)
                .ok_or_else(|| State.into())
        })
        .transpose()
}

fn check_device_colors(graph: &NativePdfGraph, resources: Option<&Dict>) -> Result<()> {
    if let Some(colors) = resources.and_then(|dict| dict.get("ColorSpace")) {
        let colors = resolve(graph, colors)
            .and_then(PdfValue::as_dictionary)
            .ok_or(State)?;
        if ["DefaultGray", "DefaultRGB", "DefaultCMYK"]
            .iter()
            .any(|key| colors.contains_key(*key))
        {
            return Err(State.into());
        }
    }
    Ok(())
}

fn contents(graph: &NativePdfGraph, page: PdfObjectRef) -> Result<Vec<PdfObjectRef>> {
    let Some(value) = page_dict(graph, page)?.get("Contents") else {
        return Ok(Vec::new());
    };
    let value_resolved = resolve(graph, value).ok_or(State)?;
    match value_resolved {
        PdfValue::Null => Ok(Vec::new()),
        PdfValue::Stream(_) => Ok(vec![value.as_reference().ok_or(State)?]),
        PdfValue::Array(items) if items.len() <= MAX_OBJECTS_PER_PAGE => items
            .iter()
            .map(|item| item.as_reference().ok_or_else(|| State.into()))
            .collect(),
        _ => Err(State.into()),
    }
}

fn name<'a>(dict: &'a Dict, key: &str) -> Option<&'a str> {
    dict.get(key).and_then(PdfValue::as_name)
}

fn number(value: &PdfValue) -> Option<f64> {
    match value {
        PdfValue::ExactReal(text) => text.parse::<f64>().ok().filter(|n| n.abs() <= 1_000_000.),
        _ => value
            .as_integer()
            .filter(|n| n.unsigned_abs() <= 1_000_000)
            .map(|n| n as f64),
    }
}

fn array<const N: usize>(value: Option<&PdfValue>) -> Option<[f64; N]> {
    let values = value?.as_array()?;
    if values.len() != N {
        return None;
    }
    let mut result = [0.; N];
    for (dest, value) in result.iter_mut().zip(values) {
        *dest = number(value)?;
    }
    Some(result)
}

fn image_profile(image: &PdfStream) -> Result<NativeObjectProfile> {
    if image.dict.keys().any(|key| {
        !matches!(
            key.as_str(),
            "Type"
                | "Subtype"
                | "Length"
                | "Width"
                | "Height"
                | "BitsPerComponent"
                | "ColorSpace"
                | "Filter"
                | "Interpolate"
        )
    }) || image
        .dict
        .get("BitsPerComponent")
        .and_then(PdfValue::as_integer)
        != Some(8)
        || (image.dict.contains_key("Filter") && name(&image.dict, "Filter") != Some("FlateDecode"))
    {
        return Err(Profile.into());
    }
    let (profile, channels) = match name(&image.dict, "ColorSpace") {
        Some("DeviceGray") => (NativeObjectProfile::RasterGray8, 1),
        Some("DeviceRGB") => (NativeObjectProfile::RasterRgb8, 3),
        _ => return Err(Profile.into()),
    };
    let dimensions = ["Width", "Height"].map(|key| {
        image
            .dict
            .get(key)
            .and_then(PdfValue::as_integer)
            .filter(|n| (1..=i64::from(NATIVE_MAX_IMAGE_DIMENSION)).contains(n))
    });
    let [Some(width), Some(height)] = dimensions else {
        return Err(Profile.into());
    };
    let pixels = (width * height) as u64;
    if pixels > NATIVE_MAX_IMAGE_PIXELS || pixels * channels != image.data.len() as u64 {
        return Err(Profile.into());
    }
    Ok(profile)
}

#[derive(Clone, Copy)]
struct GraphicsState {
    matrix: Matrix,
    stroke_width: f64,
}

#[derive(Default)]
struct Path {
    start: usize,
    points: Vec<Point>,
    rectangle: bool,
    closed: bool,
}

impl Path {
    fn local_bounds(&self, stroke: bool, width: f64) -> Result<(NativeObjectProfile, [f64; 4])> {
        let profile = match (self.rectangle, self.points.len(), self.closed || !stroke) {
            (true, 2, _) => NativeObjectProfile::Rectangle,
            (false, 2, false) => NativeObjectProfile::Line,
            (false, 3..=NATIVE_MAX_GRAPHIC_POINTS, true) => NativeObjectProfile::Polygon,
            _ => return Err(Profile.into()),
        };
        let mut raw = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for &[x, y] in &self.points {
            raw = [raw[0].min(x), raw[1].min(y), raw[2].max(x), raw[3].max(y)];
        }
        if stroke {
            if width <= 0. {
                return Err(Profile.into());
            }
            let padding = if profile == NativeObjectProfile::Polygon {
                10. * width
            } else {
                0.5 * width
            };
            raw = [
                raw[0] - padding,
                raw[1] - padding,
                raw[2] + padding,
                raw[3] + padding,
            ];
        }
        if !positive(raw) {
            return Err(Geometry.into());
        }
        Ok((profile, raw))
    }
}

#[derive(Clone)]
struct Frame {
    stream: PdfObjectRef,
    span: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct Signature {
    profile: NativeObjectProfile,
    bounds: Bounds,
    payload: [u8; 32],
}

struct Found {
    slot: usize,
    forms: Vec<Frame>,
    leaf: Frame,
    matrix: Matrix,
    local_bounds: [f64; 4],
    clip: [f64; 4],
    signature: Signature,
}

impl Found {
    fn view(&self, page: NativePageId, binding: NativeBinding, hash: [u8; 32]) -> NativeObjectView {
        let mut id = blake3::Hasher::new();
        id.update(b"flowpdf:native-object:v1");
        id.update(&binding.epoch);
        id.update(&binding.session_id.to_be_bytes());
        id.update(&binding.origin_hash);
        id.update(&binding.revision.to_be_bytes());
        id.update(&hash);
        id.update(&page.0.to_be_bytes());
        id.update(&(self.slot as u64).to_be_bytes());
        for frame in self.forms.iter().chain(std::iter::once(&self.leaf)) {
            id.update(&frame.stream.object_number.to_be_bytes());
            id.update(&frame.stream.generation.to_be_bytes());
            id.update(&(frame.span.start as u64).to_be_bytes());
            id.update(&(frame.span.end as u64).to_be_bytes());
        }
        let profile = self.signature.profile;
        NativeObjectView {
            id: *id.finalize().as_bytes(),
            kind: profile.kind(),
            profile,
            bounds: self.signature.bounds,
            operations: [
                NativeObjectOperation::Replace,
                NativeObjectOperation::Move,
                NativeObjectOperation::Remove,
            ],
        }
    }
}

struct Scan<'a> {
    graph: &'a NativePdfGraph,
    work: &'a mut usize,
    found: Vec<Found>,
    path: Vec<Frame>,
    slot: usize,
}

fn operands<const N: usize>(
    args: &[(ContentToken, Range<usize>)],
    data: &[u8],
) -> Result<[f64; N]> {
    if args.len() != N {
        return Err(State.into());
    }
    let mut values = [0.; N];
    for (dest, (token, span)) in values.iter_mut().zip(args) {
        if !matches!(token, ContentToken::Number(_) | ContentToken::Integer(_)) {
            return Err(State.into());
        }
        *dest = std::str::from_utf8(&data[span.clone()])
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.abs() <= 1_000_000.)
            .ok_or(Geometry)?;
    }
    Ok(values)
}

impl<'a> Scan<'a> {
    fn record(
        &mut self,
        leaf: Frame,
        matrix: Matrix,
        clip: [f64; 4],
        local_bounds: [f64; 4],
        profile: NativeObjectProfile,
        payload: [u8; 32],
    ) -> Result<()> {
        if self.found.len() >= MAX_OBJECTS_PER_PAGE {
            return Err(State.into());
        }
        let rect = matrix.rect(local_bounds);
        if contains(clip, rect) {
            self.found.push(Found {
                slot: self.slot,
                forms: self.path.clone(),
                leaf,
                matrix,
                local_bounds,
                clip,
                signature: Signature {
                    profile,
                    bounds: bounds(rect)?,
                    payload,
                },
            });
        }
        Ok(())
    }

    fn interpret(
        &mut self,
        reference: PdfObjectRef,
        resources: Option<&'a Dict>,
        mut state: GraphicsState,
        clip: [f64; 4],
    ) -> Result<GraphicsState> {
        let source = stream(self.graph, reference)?;
        check_device_colors(self.graph, resources)?;
        if source.data.len() > MAX_CONTENT_BYTES
            || ["F", "FFilter", "FDecodeParms", "DecodeParms"]
                .iter()
                .any(|key| !matches!(source.dict.get(*key), None | Some(PdfValue::Null)))
        {
            return Err(State.into());
        }
        spend(self.work, 1 + source.data.len())?;
        let mut cursor = ContentCursor::new(
            &source.data,
            PdfReaderLimits {
                max_content_tokens: 16_384,
                ..PdfReaderLimits::default()
            },
        );
        let mut stack = Vec::new();
        let mut args = Vec::new();
        let mut path = Path::default();
        let mut in_text = false;
        while let Some((token, offset)) = cursor.next().map_err(|_| State)? {
            let end = cursor.position();
            let ContentToken::Word(operator) = token else {
                if args.len() >= 8 {
                    return Err(State.into());
                }
                args.push((token, offset..end));
                continue;
            };
            let start = args.first().map_or(offset, |(_, span)| span.start);
            if in_text {
                match operator.as_str() {
                    "ET" if args.is_empty() => in_text = false,
                    "Tr" => {
                        let [mode] = operands(&args, &source.data)?;
                        if mode != 0. {
                            return Err(State.into());
                        }
                    }
                    "Tf" | "Tm" | "Td" | "TD" | "T*" | "TL" | "Tc" | "Tw" | "Tz" | "Ts" | "Tj"
                    | "TJ" | "'" | "\"" => {}
                    _ => return Err(State.into()),
                }
                args.clear();
                continue;
            }
            let constructing = !path.points.is_empty();
            match operator.as_str() {
                "m" if !constructing => {
                    path.start = start;
                    path.points.push(operands(&args, &source.data)?);
                }
                "l" if constructing
                    && !path.rectangle
                    && !path.closed
                    && path.points.len() < NATIVE_MAX_GRAPHIC_POINTS =>
                {
                    path.points.push(operands(&args, &source.data)?);
                }
                "re" if !constructing => {
                    let [x, y, w, h] = operands(&args, &source.data)?;
                    if w == 0. || h == 0. {
                        return Err(Geometry.into());
                    }
                    path = Path {
                        start,
                        points: vec![[x, y], [x + w, y + h]],
                        rectangle: true,
                        closed: true,
                    };
                }
                "h" if args.is_empty() && constructing && !path.closed => path.closed = true,
                "n" if args.is_empty() => path = Path::default(),
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
                    if args.is_empty() && constructing =>
                {
                    let paint = operator.as_bytes()[0];
                    path.closed |= matches!(paint, b's' | b'b');
                    let fill = !matches!(paint, b'S' | b's');
                    let stroke = !matches!(paint, b'f' | b'F');
                    if fill && path.points.len() == 2 && !path.rectangle {
                        return Err(Profile.into());
                    }
                    if fill {
                        path.closed = true;
                    }
                    let (profile, local) = path.local_bounds(stroke, state.stroke_width)?;
                    let span = path.start..end;
                    let hash = *blake3::hash(&source.data[span.clone()]).as_bytes();
                    self.record(
                        Frame {
                            stream: reference,
                            span,
                        },
                        state.matrix,
                        clip,
                        local,
                        profile,
                        hash,
                    )?;
                    path = Path::default();
                }
                _ if constructing => return Err(State.into()),
                "q" if args.is_empty() && stack.len() < 32 => stack.push(state),
                "Q" if args.is_empty() => state = stack.pop().ok_or(State)?,
                "cm" => {
                    state.matrix = Matrix(operands(&args, &source.data)?)
                        .checked()?
                        .then(state.matrix)?
                }
                "w" => {
                    let [width] = operands(&args, &source.data)?;
                    if !(0. ..=1000.).contains(&width) {
                        return Err(Geometry.into());
                    }
                    state.stroke_width = width;
                }
                "g" | "G" => {
                    let _: [f64; 1] = operands(&args, &source.data)?;
                }
                "rg" | "RG" => {
                    let _: [f64; 3] = operands(&args, &source.data)?;
                }
                "k" | "K" => {
                    let _: [f64; 4] = operands(&args, &source.data)?;
                }
                "BT" if args.is_empty() => in_text = true,
                "Do" => {
                    let [(ContentToken::Name(name), _)] = args.as_slice() else {
                        return Err(State.into());
                    };
                    if !name.bytes().all(|b| b.is_ascii_graphic() && b != b'#') {
                        return Err(State.into());
                    }
                    self.invoke(
                        resources,
                        name,
                        Frame {
                            stream: reference,
                            span: start..end,
                        },
                        state,
                        clip,
                    )?;
                }
                _ => return Err(State.into()),
            }
            args.clear();
        }
        if !stack.is_empty() || !args.is_empty() || !path.points.is_empty() || in_text {
            return Err(State.into());
        }
        Ok(state)
    }

    fn invoke(
        &mut self,
        parent: Option<&'a Dict>,
        resource: &str,
        frame: Frame,
        mut state: GraphicsState,
        clip: [f64; 4],
    ) -> Result<()> {
        let target = xobjects(self.graph, parent)?
            .and_then(|dict| dict.get(resource))
            .and_then(PdfValue::as_reference)
            .ok_or(State)?;
        if !self.graph.reverse.contains_key(&target) {
            return Err(State.into());
        }
        let source = stream(self.graph, target)?;
        match name(&source.dict, "Subtype") {
            Some("Image") => {
                let profile = image_profile(source)?;
                spend(self.work, 1 + source.encoded.len() / 16)?;
                let mut payload = blake3::Hasher::new();
                for key in ["Width", "Height"] {
                    payload.update(&source.dict[key].as_integer().ok_or(Profile)?.to_be_bytes());
                }
                payload.update(&source.encoded);
                self.record(
                    frame,
                    state.matrix,
                    clip,
                    [0., 0., 1., 1.],
                    profile,
                    *payload.finalize().as_bytes(),
                )
            }
            Some("Form") => {
                if self.path.len() >= MAX_FORM_DEPTH
                    || self.path.iter().any(|ancestor| ancestor.stream == target)
                    || frame.stream == target
                    || source.dict.keys().any(|key| {
                        !matches!(
                            key.as_str(),
                            "Type"
                                | "Subtype"
                                | "Length"
                                | "Filter"
                                | "BBox"
                                | "Matrix"
                                | "Resources"
                                | "FormType"
                        )
                    })
                    || source
                        .dict
                        .get("FormType")
                        .is_some_and(|v| v.as_integer() != Some(1))
                {
                    return Err(State.into());
                }
                if source.dict.contains_key("Matrix") {
                    let matrix =
                        Matrix(array(source.dict.get("Matrix")).ok_or(State)?).checked()?;
                    state.matrix = matrix.then(state.matrix)?;
                }
                let local = array(source.dict.get("BBox"))
                    .filter(|rect| positive(*rect))
                    .ok_or(State)?;
                let mut form_clip = state.matrix.rect(local);
                form_clip = [
                    clip[0].max(form_clip[0]),
                    clip[1].max(form_clip[1]),
                    clip[2].min(form_clip[2]),
                    clip[3].min(form_clip[3]),
                ];
                let effective = match source.dict.get("Resources") {
                    Some(value) => Some(
                        resolve(self.graph, value)
                            .and_then(PdfValue::as_dictionary)
                            .ok_or(State)?,
                    ),
                    None => parent,
                };
                self.path.push(frame);
                let result = self.interpret(target, effective, state, form_clip);
                self.path.pop();
                result.map(|_| ())
            }
            _ => Err(Profile.into()),
        }
    }
}

fn scan(graph: &NativePdfGraph, page: &NativePdfNotePage, work: &mut usize) -> Result<Vec<Found>> {
    let rect = page.geometry.ok_or(Geometry)?.bounds().map(|n| n as f64);
    if !positive(rect) {
        return Err(Geometry.into());
    }
    let resources = resources(graph, page.object)?;
    check_device_colors(graph, resources)?;
    if page_dict(graph, page.object)?.contains_key("Group") {
        return Err(State.into());
    }
    let mut scan = Scan {
        graph,
        work,
        found: Vec::new(),
        path: Vec::new(),
        slot: 0,
    };
    let mut state = GraphicsState {
        matrix: Matrix::IDENTITY,
        stroke_width: 1.,
    };
    let slots = contents(graph, page.object)?;
    for (slot, &reference) in slots.iter().enumerate() {
        // @invariant: A preceding slot cannot swallow tokens or join them to the next slot.
        if slot + 1 < slots.len() {
            let data = &stream(graph, reference)?.data;
            if !data.is_empty() && !data.ends_with(b"\n") && !data.ends_with(b"\r") {
                return Err(State.into());
            }
        }
        scan.slot = slot;
        state = scan.interpret(reference, resources, state, rect)?;
    }
    Ok(scan.found)
}

pub(super) fn project(
    graph: &NativePdfGraph,
    pages: &[NativePdfNotePage],
    hash: [u8; 32],
    binding: NativeBinding,
    remaining: &mut usize,
) -> Result<Vec<NativeObjectPage>> {
    let mut work = graph.remaining_work();
    let mut result = Vec::with_capacity(pages.len());
    spend(remaining, pages.len() * 128)?;
    for page in pages {
        let (found, refusal) = match scan(graph, page, &mut work) {
            Ok(found) => (found, None),
            Err(NativeSessionError::Object(reason)) => (Vec::new(), Some(reason)),
            Err(NativeSessionError::Budget) => (Vec::new(), Some(State)),
            Err(error) => return Err(error),
        };
        spend(remaining, found.len() * 256)?;
        result.push(NativeObjectPage {
            page: page.id,
            can_add: refusal.is_none()
                && found.len() < MAX_OBJECTS_PER_PAGE
                && contents(graph, page.object)?.len() < MAX_OBJECTS_PER_PAGE,
            objects: found
                .iter()
                .map(|item| item.view(page.id, binding, hash))
                .collect(),
            refusal,
        });
    }
    Ok(result)
}

fn graphic_source(graphic: &NativeGraphic) -> Result<(String, NativeObjectProfile, [f64; 4])> {
    use NativeGraphicShape::{Line, Polygon, Rectangle};

    let count = graphic.points.len();
    let stroked = graphic.stroke.is_some();
    let filled = graphic.fill.is_some();
    let valid_count = match graphic.shape {
        Line | Rectangle => count == 2,
        Polygon => (3..=NATIVE_MAX_GRAPHIC_POINTS).contains(&count),
    };
    if !valid_count
        || !(stroked || filled)
        || (matches!(graphic.shape, Line) && filled)
        || (stroked && !(1..=NATIVE_MAX_STROKE_WIDTH).contains(&graphic.stroke_width))
        || (!stroked && graphic.stroke_width != 0)
        || graphic
            .points
            .iter()
            .flatten()
            .any(|n| n.unsigned_abs() > MAX_COORDINATE as u64)
    {
        return Err(Geometry.into());
    }
    let points: Vec<_> = graphic
        .points
        .iter()
        .map(|point| point.map(|n| n as f64 / UNIT))
        .collect();
    if points.windows(2).any(|p| p[0] == p[1])
        || (matches!(graphic.shape, Polygon) && points.first() == points.last())
    {
        return Err(Geometry.into());
    }
    let path = Path {
        start: 0,
        points,
        rectangle: matches!(graphic.shape, Rectangle),
        closed: !matches!(graphic.shape, Line),
    };
    let width = graphic.stroke_width as f64 / UNIT;
    let (profile, local_bounds) = path.local_bounds(stroked, width)?;
    let mut source = String::from("q\n");
    for (color, operator) in [(graphic.fill, "rg"), (graphic.stroke, "RG")] {
        if let Some([r, g, b]) = color {
            let [r, g, b] = [r, g, b].map(|n| f64::from(n) / 255.);
            writeln!(source, "{r:.9} {g:.9} {b:.9} {operator}").expect("write to string");
        }
    }
    if stroked {
        writeln!(source, "{width:.3} w").expect("write to string");
    }
    if path.rectangle {
        let [[x, y], [r, t]] = [path.points[0], path.points[1]];
        if x >= r || y >= t {
            return Err(Geometry.into());
        }
        let (w, h) = (r - x, t - y);
        writeln!(source, "{x:.3} {y:.3} {w:.3} {h:.3} re").expect("write to string");
    } else {
        for (index, [x, y]) in path.points.into_iter().enumerate() {
            let operator = if index == 0 { "m" } else { "l" };
            writeln!(source, "{x:.3} {y:.3} {operator}").expect("write to string");
        }
        if path.closed {
            source.push_str("h\n");
        }
    }
    source.push_str(match (filled, stroked) {
        (true, true) => "B\nQ",
        (true, false) => "f\nQ",
        _ => "S\nQ",
    });
    Ok((source, profile, local_bounds))
}

fn image_source(hex: &str, graph: &NativePdfGraph, remaining: &mut usize) -> Result<PdfStream> {
    fn nibble(b: u8) -> Result<u8> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(Asset.into()),
        }
    }
    let bytes = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(nibble(pair[0])? << 4 | nibble(pair[1])?))
        .collect::<Result<Vec<_>>>()?;
    let format = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ImageFormat::Png
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        ImageFormat::Jpeg
    } else {
        return Err(Asset.into());
    };
    let mut limits = Limits::default();
    limits.max_image_width = Some(NATIVE_MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(NATIVE_MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(*remaining as u64);
    let mut reader = ImageReader::with_format(Cursor::new(bytes.as_slice()), format);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| Asset)?;
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > NATIVE_MAX_IMAGE_PIXELS {
        return Err(Asset.into());
    }
    let color = decoder.color_type();
    if !matches!(color, image::ColorType::L8 | image::ColorType::Rgb8)
        || decoder.original_color_type() == image::ExtendedColorType::Cmyk8
    {
        return Err(Asset.into());
    }
    let samples = usize::try_from(decoder.total_bytes()).map_err(|_| NativeSessionError::Budget)?;
    if samples > graph.limits.max_decoded_stream_bytes || samples > graph.remaining_decoded() {
        return Err(NativeSessionError::Budget);
    }
    spend(remaining, samples * 3 + 4096)?;
    let mut data = vec![0; samples];
    decoder.read_image(&mut data).map_err(|_| Asset)?;
    let encoded = miniz_oxide::deflate::compress_to_vec_zlib(&data, 6);
    if encoded.len() > graph.limits.max_output_bytes {
        return Err(NativeSessionError::Budget);
    }
    let color_space = match color {
        image::ColorType::L8 => "DeviceGray",
        _ => "DeviceRGB",
    };
    Ok(PdfStream {
        dict: BTreeMap::from([
            ("Type".into(), PdfValue::Name("XObject".into())),
            ("Subtype".into(), PdfValue::Name("Image".into())),
            ("Width".into(), PdfValue::Integer(width.into())),
            ("Height".into(), PdfValue::Integer(height.into())),
            ("BitsPerComponent".into(), PdfValue::Integer(8)),
            ("ColorSpace".into(), PdfValue::Name(color_space.into())),
            ("Filter".into(), PdfValue::Name("FlateDecode".into())),
            ("Length".into(), PdfValue::Integer(encoded.len() as i64)),
        ]),
        data,
        encoded,
        source_offset: 0,
    })
}

fn value_cost(value: &PdfValue, remaining: &mut usize) -> Result<()> {
    spend(remaining, 128)?;
    match value {
        PdfValue::Dictionary(dict) => dict_cost(dict, remaining)?,
        PdfValue::Array(items) => {
            for item in items {
                value_cost(item, remaining)?;
            }
        }
        PdfValue::String(bytes) => spend(remaining, bytes.len())?,
        PdfValue::Name(text) | PdfValue::ExactReal(text) => spend(remaining, text.len())?,
        PdfValue::Stream(_) => return Err(State.into()),
        _ => {}
    }
    Ok(())
}

fn dict_cost(dict: &Dict, remaining: &mut usize) -> Result<()> {
    for (key, value) in dict {
        spend(remaining, key.len())?;
        value_cost(value, remaining)?;
    }
    Ok(())
}

fn resource_override(
    graph: &NativePdfGraph,
    effective: Option<&Dict>,
    reference: PdfObjectRef,
    remaining: &mut usize,
) -> Result<(Dict, String)> {
    let mut xobjects = match xobjects(graph, effective)? {
        Some(dict) => {
            dict_cost(dict, remaining)?;
            dict.clone()
        }
        None => Dict::new(),
    };
    if xobjects.len() >= 4096 {
        return Err(NativeSessionError::Budget);
    }
    let resource = (1..=xobjects.len() + 1)
        .map(|index| format!("FlowObject{index}"))
        .find(|name| !xobjects.contains_key(name))
        .ok_or(NativeSessionError::Budget)?;
    xobjects.insert(resource.clone(), PdfValue::Reference(reference));
    let mut copy = if let Some(dict) = effective {
        dict_cost(dict, remaining)?;
        dict.clone()
    } else {
        Dict::new()
    };
    copy.insert("XObject".into(), PdfValue::Dictionary(xobjects));
    Ok((copy, resource))
}

fn allocate(graph: &mut NativePdfGraph, value: PdfValue) -> Result<PdfObjectRef> {
    let object_number = graph
        .objects
        .keys()
        .next_back()
        .and_then(|reference| reference.object_number.checked_add(1))
        .ok_or(NativeSessionError::Budget)?;
    let reference = PdfObjectRef {
        object_number,
        generation: 0,
    };
    graph.objects.insert(reference, value);
    Ok(reference)
}

fn edited_stream(
    graph: &NativePdfGraph,
    frame: Option<&Frame>,
    replacement: &[u8],
    resource_override: Option<Dict>,
    remaining: &mut usize,
) -> Result<PdfStream> {
    let source = frame.map(|frame| stream(graph, frame.stream)).transpose()?;
    let size = frame
        .zip(source)
        .map_or(0, |(frame, source)| source.data.len() - frame.span.len())
        + replacement.len()
        + 2;
    if size > MAX_CONTENT_BYTES || size > graph.limits.max_decoded_stream_bytes {
        return Err(NativeSessionError::Budget);
    }
    spend(remaining, size * 2 + 1024)?;
    let mut dict = match source {
        Some(source) => {
            dict_cost(&source.dict, remaining)?;
            source.dict.clone()
        }
        None => Dict::new(),
    };
    if let Some(resources) = resource_override {
        dict.insert("Resources".into(), PdfValue::Dictionary(resources));
    }
    let mut data = Vec::with_capacity(size);
    if let Some((frame, source)) = frame.zip(source) {
        data.extend_from_slice(&source.data[..frame.span.start]);
    }
    data.push(b'\n');
    data.extend_from_slice(replacement);
    data.push(b'\n');
    if let Some((frame, source)) = frame.zip(source) {
        data.extend_from_slice(&source.data[frame.span.end..]);
    }
    dict.remove("Filter");
    dict.remove("DecodeParms");
    dict.insert("Length".into(), PdfValue::Integer(data.len() as i64));
    Ok(PdfStream {
        dict,
        encoded: data.clone(),
        data,
        source_offset: 0,
    })
}

fn retain_reachable(graph: &mut NativePdfGraph, remaining: &mut usize) -> Result<()> {
    let mut work = graph.remaining_work();
    let mut pending = Vec::new();
    for value in graph.trailer.values() {
        collect_refs(value, &mut pending, &mut work, remaining)?;
    }
    let mut reachable = BTreeSet::new();
    let mut reverse = BTreeMap::<_, BTreeSet<_>>::new();
    while let Some(reference) = pending.pop() {
        if !reachable.insert(reference) {
            continue;
        }
        if reachable.len() > graph.limits.max_objects {
            return Err(NativeSessionError::Budget);
        }
        let mut children = Vec::new();
        collect_refs(
            graph.objects.get(&reference).ok_or(State)?,
            &mut children,
            &mut work,
            remaining,
        )?;
        for child in children {
            reverse.entry(child).or_default().insert(reference);
            pending.push(child);
        }
    }
    graph
        .objects
        .retain(|reference, _| reachable.contains(reference));
    graph.reverse = reverse;
    Ok(())
}

fn invocation_resources<'a>(
    graph: &'a NativePdfGraph,
    page: PdfObjectRef,
    found: Option<&Found>,
) -> Result<Vec<Option<&'a Dict>>> {
    let mut result = vec![resources(graph, page)?];
    if let Some(found) = found {
        for form in found
            .forms
            .iter()
            .skip(1)
            .map(|frame| frame.stream)
            .chain((!found.forms.is_empty()).then_some(found.leaf.stream))
        {
            let parent = result.last().copied().flatten();
            let resources = match stream(graph, form)?.dict.get("Resources") {
                Some(value) => Some(
                    resolve(graph, value)
                        .and_then(PdfValue::as_dictionary)
                        .ok_or(State)?,
                ),
                None => parent,
            };
            result.push(resources);
        }
    }
    Ok(result)
}

pub(super) struct ExpectedObject {
    page: NativePageId,
    objects: Vec<Signature>,
    changed: Option<usize>,
}

pub(super) fn stage(
    graph: &mut NativePdfGraph,
    page: &NativePdfNotePage,
    binding: NativeBinding,
    output: [u8; 32],
    command: &NativeObjectCommand,
) -> Result<(ExpectedObject, usize)> {
    use NativeObjectCommand::*;
    use NativeSessionError::{Postcondition, TargetUnavailable};

    let mut work = graph.remaining_work();
    let found = scan(graph, page, &mut work)?;
    let index = command
        .target()
        .map(|target| -> Result<usize> {
            let index = found
                .iter()
                .position(|item| item.view(page.id, binding, output).id == target.id)
                .ok_or(TargetUnavailable)?;
            if found[index].signature.profile != target.profile {
                return Err(Profile.into());
            }
            Ok(index)
        })
        .transpose()?;
    let item = index.map(|i| &found[i]);
    let page_clip = page.geometry.ok_or(Geometry)?.bounds().map(|n| n as f64);
    let clip = item.map_or(page_clip, |item| item.clip);
    let matrix = item.map_or(Matrix::IDENTITY, |item| item.matrix);
    let mut remaining = graph.remaining_retained();
    spend(&mut remaining, command.validate_size()? + 8192)?;

    let mut resource = None;
    let (mut replacement, expected) = match command {
        AddGraphic { graphic, .. } | ReplaceGraphic { graphic, .. } => {
            if item.is_some_and(|item| item.signature.profile.kind() != NativeObjectKind::Graphic) {
                return Err(Profile.into());
            }
            let (source, profile, rect) = graphic_source(graphic)?;
            let inverse = matrix.inverse()?;
            (
                format!("q\n{}\n{source}\nQ", inverse.operator()).into_bytes(),
                Some((profile, rect)),
            )
        }
        AddImage { asset_hex, .. } | ReplaceImage { asset_hex, .. } => {
            if item.is_some_and(|item| item.signature.profile.kind() != NativeObjectKind::Image) {
                return Err(Profile.into());
            }
            let rect = match command {
                AddImage { bounds, .. } => page_space(*bounds),
                _ => matrix.rect([0., 0., 1., 1.]),
            };
            if !positive(rect) || !contains(clip, rect) {
                return Err(Geometry.into());
            }
            let image = image_source(asset_hex, graph, &mut remaining)?;
            let profile = image_profile(&image)?;
            resource = Some(allocate(graph, PdfValue::Stream(image))?);
            (Vec::new(), Some((profile, rect)))
        }
        MoveObject { transform, .. } => {
            let item = item.ok_or(TargetUnavailable)?;
            let transform = Matrix(*transform).checked()?;
            let local = matrix.then(transform)?.then(matrix.inverse()?)?;
            let original = &stream(graph, item.leaf.stream)?.data[item.leaf.span.clone()];
            spend(&mut remaining, original.len() + 256)?;
            let mut source = format!("q\n{}\n", local.operator()).into_bytes();
            source.extend_from_slice(original);
            source.extend_from_slice(b"\nQ");
            (
                source,
                Some((
                    item.signature.profile,
                    matrix.then(transform)?.rect(item.local_bounds),
                )),
            )
        }
        RemoveObject { .. } => (Vec::new(), None),
    };
    let expected = expected
        .map(|(profile, rect)| -> Result<_> {
            if !contains(clip, rect) {
                return Err(Geometry.into());
            }
            Ok((profile, bounds(rect)?))
        })
        .transpose()?;

    let depth = item.map_or(0, |item| item.forms.len());
    let mut override_resources = None;
    if let Some(reference) = resource {
        let effective = invocation_resources(graph, page.object, item)?;
        let (dict, name) = resource_override(graph, effective[depth], reference, &mut remaining)?;
        override_resources = Some(dict);
        replacement = match command {
            AddImage { bounds, .. } => {
                let [l, b, r, t] = page_space(*bounds);
                let placement = Matrix([r - l, 0., 0., t - b, l, b]).checked()?;
                format!("q\n{}\n/{name} Do\nQ", placement.operator()).into_bytes()
            }
            _ => format!("/{name} Do").into_bytes(),
        };
    }
    let root = if depth == 0 {
        edited_stream(
            graph,
            item.map(|item| &item.leaf),
            &replacement,
            None,
            &mut remaining,
        )?
    } else {
        let item = item.ok_or(TargetUnavailable)?;
        let leaf = edited_stream(
            graph,
            Some(&item.leaf),
            &replacement,
            override_resources.take(),
            &mut remaining,
        )?;
        let mut child = allocate(graph, PdfValue::Stream(leaf))?;
        let mut root = None;
        for (level, frame) in item.forms.iter().enumerate().rev() {
            let effective = invocation_resources(graph, page.object, Some(item))?;
            let (dict, name) = resource_override(graph, effective[level], child, &mut remaining)?;
            let invocation = format!("/{name} Do");
            let stream = if level == 0 {
                override_resources = Some(dict);
                edited_stream(
                    graph,
                    Some(frame),
                    invocation.as_bytes(),
                    None,
                    &mut remaining,
                )?
            } else {
                edited_stream(
                    graph,
                    Some(frame),
                    invocation.as_bytes(),
                    Some(dict),
                    &mut remaining,
                )?
            };
            if level == 0 {
                root = Some(stream);
            } else {
                child = allocate(graph, PdfValue::Stream(stream))?;
            }
        }
        root.ok_or(Postcondition)?
    };
    let mut slots = contents(graph, page.object)?;
    let root = allocate(graph, PdfValue::Stream(root))?;
    if let Some(item) = item {
        slots[item.slot] = root;
    } else {
        slots.insert(0, root);
    }
    let Some(PdfValue::Dictionary(dict)) = graph.objects.get_mut(&page.object) else {
        return Err(TargetUnavailable);
    };
    let mut slots: Vec<_> = slots.into_iter().map(PdfValue::Reference).collect();
    let contents = if slots.len() == 1 {
        slots.remove(0)
    } else {
        PdfValue::Array(slots)
    };
    dict.insert("Contents".into(), contents);
    if let Some(resources) = override_resources {
        dict.insert("Resources".into(), PdfValue::Dictionary(resources));
    }
    retain_reachable(graph, &mut remaining)?;

    let changed = expected.map(|_| index.unwrap_or(0));
    let next = scan(graph, page, &mut work)?;
    let next: Vec<_> = next.into_iter().map(|item| item.signature).collect();
    if let Some((profile, rect)) = expected {
        let actual = next
            .get(changed.ok_or(Postcondition)?)
            .ok_or(Postcondition)?;
        if actual.profile != profile || actual.bounds != rect {
            return Err(Postcondition);
        }
        if matches!(command, MoveObject { .. })
            && item.is_none_or(|item| actual.payload != item.signature.payload)
        {
            return Err(Postcondition);
        }
    }
    if found
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != index)
        .map(|(_, item)| &item.signature)
        .ne(next
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != changed)
            .map(|(_, item)| item))
    {
        return Err(Postcondition);
    }
    Ok((
        ExpectedObject {
            page: page.id,
            objects: next,
            changed,
        },
        remaining,
    ))
}

pub(super) fn verify(
    graph: &NativePdfGraph,
    pages: &[NativePdfNotePage],
    views: &[NativeObjectPage],
    expected: &ExpectedObject,
) -> Result<Option<[u8; 32]>> {
    let index = expected.page.0 as usize;
    let page = pages.get(index).ok_or(NativeSessionError::Postcondition)?;
    let mut work = graph.remaining_work();
    let found = scan(graph, page, &mut work)?;
    if found
        .iter()
        .map(|item| &item.signature)
        .ne(&expected.objects)
    {
        return Err(NativeSessionError::Postcondition);
    }
    match expected.changed {
        Some(changed) => Ok(Some(
            views
                .get(index)
                .and_then(|page| page.objects.get(changed))
                .ok_or(NativeSessionError::Postcondition)?
                .id,
        )),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::pdf::native::{NativeEditLimits, NativePdfSession};
    use serde_json::json;

    #[test]
    fn graphic_staging_checks_exact_paint_span_and_accepted_snapshot() {
        let content = miniz_oxide::deflate::compress_to_vec_zlib(
            b"2 w q 10 20 30 40 re % in the path\n f Q\nBT (10 20 30 40 re f) Tj ET",
            6,
        );
        let source = super::super::tests::fixture("", &content);
        let mut session =
            NativePdfSession::open_scoped(&source, NativeEditLimits::default(), [4; 16]).unwrap();
        let before = session.snapshot();
        let view = &before.scene.object_pages[0].objects[0];
        assert_eq!(view.bounds, [10000, 20000, 40000, 60000]);
        let command = serde_json::from_value(json!({
            "kind": "moveObject",
            "target": {"page": 0, "id": view.id, "profile": view.profile},
            "transform": [0, 2, -1, 0, 100, 0],
        }))
        .unwrap();
        let receipt = session.apply_object(before.binding, 1, &command).unwrap();
        let after = session.snapshot();
        let view = &after.scene.object_pages[0].objects[0];
        assert_eq!(receipt.object, Some(view.id));
        assert_eq!(view.bounds, [40000, 20000, 80000, 80000]);
        assert!(
            session
                .output_bytes()
                .windows(b"(10 20 30 40 re f) Tj".len())
                .any(|bytes| bytes == b"(10 20 30 40 re f) Tj")
        );
        assert_eq!(session.origin_bytes(), source);
    }
}
