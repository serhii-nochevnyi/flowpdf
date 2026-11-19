use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::mem::size_of;

use miniz_oxide::deflate::compress_to_vec_zlib;

use super::super::reader::{PdfObjectRef, PdfReaderLimits, PdfStream, PdfValue, single_filter};
use super::super::scene::{ContentCursor, ContentToken, separated_content_streams};
use super::graph::{NativeEditError, NativePdfGraph, spend};
use super::text::{RedactionText, redaction_number};
use super::verifier::{NativeRedactionGeometry, NativeRedactionMark};
use super::verify_raster;
use super::verify_structure::MAX_INPUT;

const POINT: i64 = 1_000;
const MAX_COORD: i64 = 1_000_000 * POINT;
const MAX_STREAM: usize = 512 * 1024;
const MAX_FORM_DEPTH: usize = 8;
const MAX_PAGES: usize = 256;
pub(super) const MAX_MARKS: usize = 1024;

type Object = PdfObjectRef;
type Dict = BTreeMap<String, PdfValue>;
type Rect = [i64; 4];
type Digest = [u8; 32];
type Result<T> = std::result::Result<T, NativeEditError>;
use NativeEditError::{Budget, Malformed, Unsupported};

pub(super) fn valid_rect([l, b, r, t]: Rect) -> bool {
    l < r && b < t && [l, b, r, t].into_iter().all(in_range)
}

pub(super) fn contains([l, b, r, t]: Rect, inner: Rect) -> bool {
    l <= inner[0] && b <= inner[1] && r >= inner[2] && t >= inner[3]
}

fn intersects(a: Rect, b: Rect) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn in_range(value: i64) -> bool {
    (-MAX_COORD..=MAX_COORD).contains(&value)
}

fn get<'a>(dict: &'a Dict, key: &str) -> Result<&'a PdfValue> {
    dict.get(key).ok_or(Malformed)
}

fn name<'a>(dict: &'a Dict, key: &str) -> Option<&'a str> {
    dict.get(key).and_then(PdfValue::as_name)
}

fn reference(value: &PdfValue) -> Result<Object> {
    value.as_reference().ok_or(Malformed)
}

fn resolve<'a>(graph: &'a NativePdfGraph, value: &'a PdfValue) -> Result<&'a PdfValue> {
    match value {
        PdfValue::Reference(id) => graph.objects.get(id).ok_or(Malformed),
        value => Ok(value),
    }
}

fn dictionary(value: &PdfValue) -> Result<&Dict> {
    match value {
        PdfValue::Dictionary(dict) => Ok(dict),
        _ => Err(Malformed),
    }
}

fn dict(graph: &NativePdfGraph, id: Object) -> Result<&Dict> {
    dictionary(graph.objects.get(&id).ok_or(Malformed)?)
}

fn stream(graph: &NativePdfGraph, id: Object) -> Result<&PdfStream> {
    match graph.objects.get(&id) {
        Some(PdfValue::Stream(stream)) => Ok(stream),
        _ => Err(Malformed),
    }
}

fn fixed(value: &PdfValue) -> Result<i64> {
    match value {
        PdfValue::Integer(n) => n.checked_mul(POINT).filter(|n| in_range(*n)),
        PdfValue::ExactReal(raw) => redaction_number(raw.as_bytes()),
        _ => None,
    }
    .ok_or(Unsupported)
}

fn values<const N: usize>(value: &PdfValue) -> Result<[i64; N]> {
    let array: &[_; N] = value
        .as_array()
        .and_then(|array| array.try_into().ok())
        .ok_or(Malformed)?;
    let mut result = [0; N];
    for (out, value) in result.iter_mut().zip(array) {
        *out = fixed(value)?;
    }
    Ok(result)
}

fn rect(value: &PdfValue) -> Result<Rect> {
    let rect = values(value)?;
    valid_rect(rect).then_some(rect).ok_or(Unsupported)
}

struct Page<'a> {
    geometry: NativeRedactionGeometry,
    resources: Option<&'a Dict>,
}

fn page<'a>(graph: &'a NativePdfGraph, id: Object, work: &mut usize) -> Result<Page<'a>> {
    let (mut media, mut crop, mut rotation, mut resources) = (None, None, None, None);
    let mut current = Some(id);
    for _ in 0..=graph.limits.max_nesting {
        let Some(id) = current else {
            let media_box = media.ok_or(Malformed)?;
            let crop_box = crop.unwrap_or(media_box);
            if !contains(media_box, crop_box) {
                return Err(Unsupported);
            }
            return Ok(Page {
                geometry: NativeRedactionGeometry {
                    media_box,
                    crop_box,
                    rotation: rotation.unwrap_or(0),
                },
                resources,
            });
        };
        spend(work, 1)?;
        let node = dict(graph, id)?;
        if node
            .get("UserUnit")
            .is_some_and(|unit| unit.as_integer() != Some(1))
        {
            return Err(Unsupported);
        }
        if media.is_none() {
            media = node
                .get("MediaBox")
                .map(|v| rect(resolve(graph, v)?))
                .transpose()?;
        }
        if crop.is_none() {
            crop = node
                .get("CropBox")
                .map(|v| rect(resolve(graph, v)?))
                .transpose()?;
        }
        if rotation.is_none()
            && let Some(value) = node.get("Rotate")
        {
            let angle = value.as_integer().ok_or(Malformed)?.rem_euclid(360);
            if angle % 90 != 0 {
                return Err(Unsupported);
            }
            rotation = Some(angle as u16);
        }
        if resources.is_none() {
            resources = node
                .get("Resources")
                .map(|v| dictionary(resolve(graph, v)?))
                .transpose()?;
        }
        current = node.get("Parent").map(reference).transpose()?;
    }
    Err(Malformed)
}

pub(super) fn geometry(graph: &NativePdfGraph, index: usize) -> Result<NativeRedactionGeometry> {
    let id = *graph.pages.get(index).ok_or(Malformed)?;
    let mut work = graph.limits.max_nesting + 1;
    Ok(page(graph, id, &mut work)?.geometry)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Catalog,
    Tree,
    Node,
    Page,
    Contents,
    Content,
    Resources,
    Fonts,
    XObjects,
    Font,
    CidFont,
    Descriptor,
    FontFile,
    Unicode,
    Glyphs,
    XObject,
    Annots,
    Note,
    Info,
    Metadata,
    Source,
    Length,
    Box,
    Widths,
}

struct Inventory<'a> {
    graph: &'a NativePdfGraph,
    roles: BTreeMap<Object, Role>,
    pending: Vec<(Object, Role)>,
    work: usize,
    memory: usize,
}

impl<'a> Inventory<'a> {
    fn read(graph: &'a NativePdfGraph) -> Result<Self> {
        let mut inventory = Self {
            graph,
            roles: BTreeMap::new(),
            pending: Vec::new(),
            work: graph.remaining_work(),
            memory: graph.remaining_retained() / 4,
        };
        keys(&graph.trailer, &["Root", "Size", "Info", "ID"])?;
        inventory.link(get(&graph.trailer, "Root")?, Role::Catalog)?;
        if let Some(info) = graph.trailer.get("Info") {
            inventory.link(info, Role::Info)?;
        }
        if let Some(id) = graph.trailer.get("ID") {
            let [PdfValue::String(a), PdfValue::String(b)] = id.as_array().ok_or(Malformed)? else {
                return Err(Malformed);
            };
            if a.len().max(b.len()) > 128 {
                return Err(Unsupported);
            }
        }
        while let Some((id, role)) = inventory.pending.pop() {
            spend(&mut inventory.work, 1)?;
            let value = graph.objects.get(&id).ok_or(Malformed)?;
            let role = match (role, value.as_dictionary().and_then(|d| name(d, "Type"))) {
                (Role::Node, Some("Page")) => Role::Page,
                (Role::Node, Some("Pages")) => Role::Tree,
                (Role::Contents, _) if matches!(value, PdfValue::Stream(_)) => Role::Content,
                (role, _) => role,
            };
            match inventory.roles.entry(id) {
                Entry::Occupied(entry) if *entry.get() == role => continue,
                Entry::Occupied(_) => return Err(Unsupported),
                Entry::Vacant(entry) => {
                    spend(&mut inventory.memory, 128)?;
                    entry.insert(role);
                }
            }
            inventory.inspect(value, role)?;
        }
        if inventory.roles.len() != graph.objects.len() {
            return Err(Unsupported);
        }
        Ok(inventory)
    }

    fn link(&mut self, value: &PdfValue, role: Role) -> Result<()> {
        spend(&mut self.work, 1)?;
        spend(&mut self.memory, 2 * size_of::<(Object, Role)>())?;
        self.pending.push((reference(value)?, role));
        Ok(())
    }

    fn entry(&mut self, value: &PdfValue, role: Role) -> Result<()> {
        if value.as_reference().is_some() {
            self.link(value, role)
        } else {
            self.inspect(value, role)
        }
    }

    fn entries(&mut self, dict: &Dict, roles: &[(&str, Role)]) -> Result<()> {
        for &(key, role) in roles {
            if let Some(value) = dict.get(key) {
                self.entry(value, role)?;
            }
        }
        Ok(())
    }

    fn inspect(&mut self, value: &PdfValue, role: Role) -> Result<()> {
        use Role::*;
        spend(&mut self.work, 1)?;
        match role {
            Length => {
                return value
                    .as_integer()
                    .filter(|n| *n >= 0)
                    .map(|_| ())
                    .ok_or(Malformed);
            }
            Box => return rect(value).map(|_| ()),
            Widths => return numeric(value, 0, &mut self.work),
            Contents | Annots => {
                let child = if role == Contents { Content } else { Note };
                return value
                    .as_array()
                    .ok_or(Malformed)?
                    .iter()
                    .try_for_each(|value| self.link(value, child));
            }
            _ => {}
        }
        if let PdfValue::Stream(stream) = value {
            return self.inspect_stream(stream, role);
        }
        let dict = dictionary(value)?;
        spend(&mut self.work, dict.len())?;
        match role {
            Catalog => {
                typed(dict, "Catalog", &["Pages", "Metadata", "FlowPDFSource"])?;
                self.link(get(dict, "Pages")?, Tree)?;
                self.entries(dict, &[("Metadata", Metadata), ("FlowPDFSource", Source)])?;
            }
            Tree | Page => {
                if role == Tree {
                    typed(
                        dict,
                        "Pages",
                        &[
                            "Parent",
                            "Resources",
                            "MediaBox",
                            "CropBox",
                            "Rotate",
                            "Kids",
                            "Count",
                        ],
                    )?;
                    get(dict, "Count")?.as_integer().ok_or(Malformed)?;
                    for kid in get(dict, "Kids")?.as_array().ok_or(Malformed)? {
                        self.link(kid, Node)?;
                    }
                } else {
                    typed(
                        dict,
                        "Page",
                        &[
                            "Parent",
                            "Resources",
                            "MediaBox",
                            "CropBox",
                            "Rotate",
                            "Contents",
                            "Annots",
                            "UserUnit",
                            "Metadata",
                        ],
                    )?;
                    self.entries(
                        dict,
                        &[
                            ("Contents", Contents),
                            ("Annots", Annots),
                            ("Metadata", Metadata),
                        ],
                    )?;
                    if dict
                        .get("UserUnit")
                        .is_some_and(|v| v.as_integer() != Some(1))
                    {
                        return Err(Unsupported);
                    }
                }
                self.entries(
                    dict,
                    &[
                        ("Resources", Resources),
                        ("MediaBox", Box),
                        ("CropBox", Box),
                    ],
                )?;
                if let Some(parent) = dict.get("Parent") {
                    self.link(parent, Tree)?;
                }
                if dict.get("Rotate").is_some_and(|v| v.as_integer().is_none()) {
                    return Err(Unsupported);
                }
            }
            Resources => {
                keys(dict, &["Font", "XObject", "ProcSet"])?;
                self.entries(dict, &[("Font", Fonts), ("XObject", XObjects)])?;
                if let Some(value) = dict.get("ProcSet") {
                    for value in value.as_array().ok_or(Malformed)? {
                        spend(&mut self.work, 1)?;
                        if !matches!(
                            value.as_name(),
                            Some("PDF" | "Text" | "ImageB" | "ImageC" | "ImageI")
                        ) {
                            return Err(Unsupported);
                        }
                    }
                }
            }
            Fonts | XObjects => {
                for (name, value) in dict {
                    if !safe_name(name) {
                        return Err(Unsupported);
                    }
                    self.link(value, if role == Fonts { Font } else { XObject })?;
                }
            }
            Font | CidFont | Descriptor => self.inspect_font(dict, role)?,
            Note => {
                keys(dict, &["Type", "Subtype", "Rect", "Contents", "P"])?;
                if name(dict, "Subtype") != Some("Text")
                    || dict
                        .get("Type")
                        .is_some_and(|v| v.as_name() != Some("Annot"))
                    || get(dict, "Contents")?
                        .as_bytes()
                        .is_none_or(|v| v.len() > 32_770)
                {
                    return Err(Unsupported);
                }
                rect(get(dict, "Rect")?)?;
                if let Some(owner) = dict.get("P") {
                    self.link(owner, Page)?;
                }
            }
            Info => {
                keys(
                    dict,
                    &[
                        "Title",
                        "Author",
                        "Subject",
                        "Keywords",
                        "Creator",
                        "Producer",
                        "CreationDate",
                        "ModDate",
                        "Trapped",
                    ],
                )?;
                for (key, value) in dict {
                    if key == "Trapped" {
                        if !matches!(value.as_name(), Some("True" | "False" | "Unknown")) {
                            return Err(Unsupported);
                        }
                    } else if value.as_bytes().is_none() {
                        return Err(Unsupported);
                    }
                }
            }
            _ => return Err(Unsupported),
        }
        Ok(())
    }

    fn inspect_stream(&mut self, stream: &PdfStream, role: Role) -> Result<()> {
        use Role::*;
        let dict = &stream.dict;
        let fields: &[&str] = match role {
            Content | Unicode | Glyphs => &[],
            FontFile => &["Length1"],
            Metadata | Source => &["Type", "Subtype"],
            XObject if name(dict, "Subtype") == Some("Image") => &[
                "Type",
                "Subtype",
                "Width",
                "Height",
                "BitsPerComponent",
                "ColorSpace",
                "Interpolate",
            ],
            XObject if name(dict, "Subtype") == Some("Form") => {
                &["Type", "Subtype", "BBox", "Matrix", "Resources", "FormType"]
            }
            _ => return Err(Unsupported),
        };
        if dict.keys().any(|key| {
            !matches!(key.as_str(), "Length" | "Filter") && !fields.contains(&key.as_str())
        }) {
            return Err(Unsupported);
        }
        match dict.get("Filter") {
            None => {}
            Some(filter) if matches!(filter.as_name(), Some("FlateDecode" | "Fl")) => {}
            Some(filter)
                if role == XObject
                    && name(dict, "Subtype") == Some("Image")
                    && matches!(single_filter(filter), Some("DCTDecode" | "DCT")) => {}
            _ => return Err(Unsupported),
        }
        let length = get(dict, "Length")?;
        if length.as_reference().is_some() {
            self.link(length, Length)?;
        } else if length.as_integer() != i64::try_from(stream.encoded.len()).ok() {
            return Err(Malformed);
        }
        match role {
            Metadata | Source => {
                let subtype = if role == Metadata {
                    "XML"
                } else {
                    "FlowPDF.Source"
                };
                if name(dict, "Type") != Some("Metadata") || name(dict, "Subtype") != Some(subtype)
                {
                    return Err(Unsupported);
                }
            }
            Unicode => check_cmap_metadata(&stream.data, &mut self.work)?,
            FontFile => {
                if get(dict, "Length1")?.as_integer() != i64::try_from(stream.data.len()).ok() {
                    return Err(Malformed);
                }
                check_font_program(&stream.data, &mut self.work)?;
            }
            XObject => {
                if dict
                    .get("Type")
                    .is_some_and(|v| v.as_name() != Some("XObject"))
                {
                    return Err(Unsupported);
                }
                if name(dict, "Subtype") == Some("Form") {
                    form_geometry(dict)?;
                    self.entries(dict, &[("Resources", Resources)])?;
                } else {
                    Image::read(stream)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn inspect_font(&mut self, dict: &Dict, role: Role) -> Result<()> {
        use Role::*;
        match role {
            Font if name(dict, "Subtype") == Some("TrueType") => {
                typed(
                    dict,
                    "Font",
                    &[
                        "Subtype",
                        "BaseFont",
                        "Encoding",
                        "FirstChar",
                        "LastChar",
                        "Widths",
                        "FontDescriptor",
                        "ToUnicode",
                    ],
                )?;
            }
            Font if name(dict, "Subtype") == Some("Type0") => {
                typed(
                    dict,
                    "Font",
                    &[
                        "Subtype",
                        "BaseFont",
                        "Encoding",
                        "DescendantFonts",
                        "ToUnicode",
                    ],
                )?;
            }
            CidFont if name(dict, "Subtype") == Some("CIDFontType2") => {
                typed(
                    dict,
                    "Font",
                    &[
                        "Subtype",
                        "BaseFont",
                        "CIDSystemInfo",
                        "CIDToGIDMap",
                        "FontDescriptor",
                        "DW",
                        "W",
                    ],
                )?;
            }
            Descriptor => {
                typed(
                    dict,
                    "FontDescriptor",
                    &[
                        "FontName",
                        "Flags",
                        "FontBBox",
                        "ItalicAngle",
                        "Ascent",
                        "Descent",
                        "CapHeight",
                        "StemV",
                        "XHeight",
                        "Leading",
                        "AvgWidth",
                        "MaxWidth",
                        "MissingWidth",
                        "StemH",
                        "FontWeight",
                        "FontFile2",
                    ],
                )?;
            }
            _ => return Err(Unsupported),
        }
        for (key, value) in dict {
            match key.as_str() {
                "Type" | "Subtype" => {
                    value.as_name().ok_or(Malformed)?;
                }
                "BaseFont" | "FontName" => {
                    let name = value.as_name().ok_or(Malformed)?;
                    if name.is_empty()
                        || name.len() > 63
                        || !name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
                    {
                        return Err(Unsupported);
                    }
                }
                "Encoding" => {
                    let expected = if name(dict, "Subtype") == Some("Type0") {
                        "Identity-H"
                    } else {
                        "WinAnsiEncoding"
                    };
                    if value.as_name() != Some(expected) {
                        return Err(Unsupported);
                    }
                }
                "DescendantFonts" => {
                    let [child] = value.as_array().ok_or(Malformed)? else {
                        return Err(Malformed);
                    };
                    self.link(child, CidFont)?;
                }
                "CIDSystemInfo" => {
                    let info = dictionary(value)?;
                    keys(info, &["Registry", "Ordering", "Supplement"])?;
                    if get(info, "Registry")?.as_bytes() != Some(b"Adobe")
                        || get(info, "Ordering")?.as_bytes() != Some(b"Identity")
                    {
                        return Err(Unsupported);
                    }
                    if !get(info, "Supplement")?
                        .as_integer()
                        .is_some_and(|n| (0..=10).contains(&n))
                    {
                        return Err(Unsupported);
                    }
                }
                "CIDToGIDMap" if value.as_name() == Some("Identity") => {}
                "CIDToGIDMap" => self.link(value, Glyphs)?,
                "FontDescriptor" => self.link(value, Descriptor)?,
                "FontFile2" => self.link(value, FontFile)?,
                "ToUnicode" => self.link(value, Unicode)?,
                "W" | "Widths" => self.entry(value, Widths)?,
                "FontBBox" => {
                    values::<4>(value)?;
                }
                _ => {
                    fixed(value)?;
                }
            }
        }
        Ok(())
    }
}

fn keys(dict: &Dict, allowed: &[&str]) -> Result<()> {
    if dict.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Unsupported);
    }
    Ok(())
}

fn typed(dict: &Dict, expected: &str, allowed: &[&str]) -> Result<()> {
    if name(dict, "Type") != Some(expected)
        || dict
            .keys()
            .any(|key| key != "Type" && !allowed.contains(&key.as_str()))
    {
        return Err(Unsupported);
    }
    Ok(())
}

fn numeric(value: &PdfValue, depth: u8, work: &mut usize) -> Result<()> {
    spend(work, 1)?;
    match value {
        PdfValue::Array(values) if depth < 2 => {
            values.iter().try_for_each(|v| numeric(v, depth + 1, work))
        }
        _ => fixed(value).map(|_| ()),
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'#')
}

fn check_cmap_metadata(data: &[u8], work: &mut usize) -> Result<()> {
    if data.len() > 128 * 1024 || !data.is_ascii() || data.contains(&b'%') {
        return Err(Unsupported);
    }
    spend(work, data.len() + 1)?;
    let mut words = data
        .split(|&b| b.is_ascii_whitespace() || b"\0<>()".contains(&b))
        .filter(|word| !word.is_empty());
    while let Some(word) = words.next() {
        match word {
            b"/CMapName" => {
                let name = words.next().ok_or(Malformed)?;
                if !matches!(
                    name,
                    b"/Example"
                        | b"/Adobe-Identity-UCS"
                        | b"/Adobe-Identity-UCS2"
                        | b"/FixtureMap"
                        | b"/Fixture-Composite"
                        | b"/Fixture-Latin"
                ) {
                    let count = name.strip_prefix(b"/FlowPDF-").ok_or(Unsupported)?;
                    if count.len() > 4
                        || count.starts_with(b"0")
                        || !std::str::from_utf8(count)
                            .ok()
                            .and_then(|n| n.parse::<usize>().ok())
                            .is_some_and(|n| (1..=4096).contains(&n))
                        || !count.iter().all(u8::is_ascii_digit)
                    {
                        return Err(Unsupported);
                    }
                }
            }
            b"/CIDSystemInfo" => {
                if words.next() != Some(b"/Registry")
                    || words.next() != Some(b"Adobe")
                    || words.next() != Some(b"/Ordering")
                    || !matches!(words.next(), Some(b"Identity" | b"UCS"))
                    || words.next() != Some(b"/Supplement")
                {
                    return Err(Unsupported);
                }
                if !matches!(
                    words.next(),
                    Some(
                        b"0" | b"1" | b"2" | b"3" | b"4" | b"5" | b"6" | b"7" | b"8" | b"9" | b"10"
                    )
                ) {
                    return Err(Unsupported);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_font_program(data: &[u8], work: &mut usize) -> Result<()> {
    let bundled = include_bytes!("../../../data/NotoSans-Regular.ttf");
    if data.len() != bundled.len() {
        return Err(Unsupported);
    }
    spend(work, data.len().div_ceil(16) + 1)?;
    if data != bundled {
        return Err(Unsupported);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Matrix([i64; 6]);

impl Matrix {
    const IDENTITY: Self = Self([POINT, 0, 0, POINT, 0, 0]);

    fn new(components: [i64; 6]) -> Result<Self> {
        let [a, b, c, d, _, _] = components;
        if !components.into_iter().all(in_range)
            || !((b == 0 && c == 0 && a * d > 0) || (a == 0 && d == 0 && b * c < 0))
        {
            return Err(Unsupported);
        }
        Ok(Self(components))
    }

    fn then(self, inner: Self) -> Result<Self> {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, x, y] = inner.0;
        let products = [
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * x + c * y + e * POINT,
            b * x + d * y + f * POINT,
        ];
        if products.iter().any(|n| n % POINT != 0) {
            return Err(Unsupported);
        }
        Self::new(products.map(|n| n / POINT))
    }

    fn numerator(self, [x, y]: Point) -> Point {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e * POINT, b * x + d * y + f * POINT]
    }

    fn bounds(self, bounds: Rect) -> Result<Rect> {
        self.round_bounds(bounds, false)
    }

    fn clip_bounds(self, bounds: Rect) -> Result<Rect> {
        self.round_bounds(bounds, true)
    }

    fn round_bounds(self, [l, b, r, t]: Rect, inward: bool) -> Result<Rect> {
        if l > r || b > t || ![l, b, r, t].into_iter().all(in_range) {
            return Err(Unsupported);
        }
        let [x, y] = self.numerator([l, b]);
        let [u, v] = self.numerator([r, t]);
        let min = if inward { ceil_point } else { floor_point };
        let max = if inward { floor_point } else { ceil_point };
        let result = [min(x.min(u)), min(y.min(v)), max(x.max(u)), max(y.max(v))];
        valid_rect(result).then_some(result).ok_or(Unsupported)
    }

    fn margin(self, [x, y]: Point) -> Point {
        let [a, b, c, d, _, _] = self.0;
        [a.abs() * x + c.abs() * y, b.abs() * x + d.abs() * y]
    }
}

fn floor_point(value: i64) -> i64 {
    value.div_euclid(POINT)
}

fn ceil_point(value: i64) -> i64 {
    -floor_point(-value)
}

fn form_geometry(dict: &Dict) -> Result<(Matrix, Rect)> {
    if dict
        .get("FormType")
        .is_some_and(|v| v.as_integer() != Some(1))
    {
        return Err(Unsupported);
    }
    let matrix = dict
        .get("Matrix")
        .map(|v| Matrix::new(values(v)?))
        .transpose()?
        .unwrap_or(Matrix::IDENTITY);
    Ok((matrix, rect(get(dict, "BBox")?)?))
}

struct Image {
    width: usize,
    height: usize,
    channels: usize,
}

impl Image {
    fn read(stream: &PdfStream) -> Result<Self> {
        let dimension = |key| {
            get(&stream.dict, key)?
                .as_integer()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| (1..=crate::asset::MAX_IMAGE_DIMENSION as usize).contains(n))
                .ok_or(Unsupported)
        };
        let width = dimension("Width")?;
        let height = dimension("Height")?;
        let channels = match name(&stream.dict, "ColorSpace") {
            Some("DeviceGray") => 1,
            Some("DeviceRGB") => 3,
            _ => return Err(Unsupported),
        };
        if width * height > 16_000_000
            || stream.data.len() != width * height * channels
            || get(&stream.dict, "BitsPerComponent")?.as_integer() != Some(8)
            || stream
                .dict
                .get("Interpolate")
                .is_some_and(|v| !matches!(v, PdfValue::Boolean(false)))
        {
            return Err(Unsupported);
        }
        Ok(Self {
            width,
            height,
            channels,
        })
    }

    // @contract: Version 1 covers every sample cell with positive-area intersection.
    fn scrub(&self, data: &mut [u8], matrix: Matrix, mark: Rect, work: &mut usize) -> Result<()> {
        let [a, b, c, d, e, f] = matrix.0;
        let (x, y) = if a != 0 {
            (
                pixel_span(mark[0], mark[2], e, a, self.width),
                pixel_span(mark[1], mark[3], f + d, -d, self.height),
            )
        } else {
            (
                pixel_span(mark[1], mark[3], f, b, self.width),
                pixel_span(mark[0], mark[2], e + c, -c, self.height),
            )
        };
        if let (Some((left, right)), Some((top, bottom))) = (x, y) {
            spend(
                work,
                (bottom - top) + ((right - left) * (bottom - top)).div_ceil(16),
            )?;
            for row in top..bottom {
                let start = (row * self.width + left) * self.channels;
                let end = (row * self.width + right) * self.channels;
                data[start..end].fill(0);
            }
        }
        Ok(())
    }
}

fn pixel_span(
    start: i64,
    end: i64,
    origin: i64,
    axis: i64,
    count: usize,
) -> Option<(usize, usize)> {
    let length = axis.abs();
    let low = (start - origin) * axis.signum();
    let high = (end - origin) * axis.signum();
    let first = low.min(high).clamp(0, length);
    let last = low.max(high).clamp(0, length);
    if first == last {
        return None;
    }
    let count = count as i64;
    Some((
        (first * count / length) as usize,
        ((last * count + length - 1) / length) as usize,
    ))
}

type UsedResources = BTreeMap<(&'static str, String), Object>;

fn used_resources(used: UsedResources) -> Dict {
    let mut groups = BTreeMap::<String, Dict>::new();
    for ((group, name), id) in used {
        groups
            .entry(group.into())
            .or_default()
            .insert(name, PdfValue::Reference(id));
    }
    groups
        .into_iter()
        .map(|(key, group)| (key, PdfValue::Dictionary(group)))
        .collect()
}

fn resolve_resource(
    graph: &NativePdfGraph,
    resources: Option<&Dict>,
    group: &str,
    key: &str,
) -> Result<Object> {
    if !safe_name(key) {
        return Err(Unsupported);
    }
    let resources = resources.ok_or(Malformed)?;
    let group = dictionary(resolve(graph, get(resources, group)?)?)?;
    reference(get(group, key)?)
}

#[derive(Clone, Copy)]
struct Graphics {
    matrix: Matrix,
    width: i64,
}

struct Path {
    start: usize,
    output: usize,
    points: Vec<[i64; 2]>,
    closed: bool,
}

type Point = [i64; 2];

impl Path {
    fn should_remove(
        mut self,
        paint: &str,
        graphics: Graphics,
        scope: Scope<'_, '_>,
        work: &mut usize,
    ) -> Result<bool> {
        let stroke = !matches!(paint, "f" | "F" | "f*");
        let fill = !matches!(paint, "S" | "s");
        let closed = self.closed || matches!(paint, "s" | "b" | "b*");
        if self.points.len() > 2 && self.points.first() == self.points.last() {
            self.points.pop();
            if stroke && !closed {
                return Err(Unsupported);
            }
        }
        if self.points.len() < 2
            || (self.points.len() < 3 && (fill || closed))
            || !self.points.iter().flatten().copied().all(in_range)
            || self.points.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(Unsupported);
        }

        let mut remove = false;
        if fill {
            spend(work, self.points.len() * self.points.len())?;
            if !strictly_convex(&self.points) {
                return Err(Unsupported);
            }
            remove |= polygon_hits(&self.points, [0; 2], graphics.matrix, scope, work, false)?;
        }
        if stroke {
            remove |= stroke_hits(&self.points, closed, graphics, scope, work)?;
        }
        Ok(remove)
    }
}

fn edge_pairs(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
}

fn difference([x, y]: Point, [u, v]: Point) -> Point {
    [x - u, y - v]
}

fn translated([x, y]: Point, [u, v]: Point) -> Point {
    [x + u, y + v]
}

fn cross([x, y]: Point, [u, v]: Point) -> i128 {
    i128::from(x) * i128::from(v) - i128::from(y) * i128::from(u)
}

fn side(a: Point, b: Point, point: Point) -> i128 {
    cross(difference(b, a), difference(point, a))
}

fn strictly_convex(points: &[Point]) -> bool {
    let orientation = side(points[0], points[1], points[2]).signum();
    orientation != 0
        && edge_pairs(points).all(|(a, b)| {
            points
                .iter()
                .copied()
                .all(|p| p == a || p == b || side(a, b, p).signum() == orientation)
        })
        && points
            .iter()
            .enumerate()
            .all(|(i, p)| !points[..i].contains(p))
}

fn polygon_hits(
    points: &[Point],
    error: Point,
    matrix: Matrix,
    scope: Scope<'_, '_>,
    work: &mut usize,
    stroke: bool,
) -> Result<bool> {
    let [l, b, r, t] = point_bounds(points);
    let [dx, dy] = error;
    let bounds = matrix.bounds([l - dx, b - dy, r + dx, t + dy])?;
    if !contains(scope.clip, bounds) {
        return Err(Unsupported);
    }

    let corners = points
        .iter()
        .map(|&point| matrix.numerator(point))
        .collect::<Vec<_>>();
    spend(work, corners.len() * corners.len())?;
    let segment = corners.len() == 2;
    let convex = !segment && strictly_convex(&corners);
    if !stroke && !convex {
        return Err(Unsupported);
    }
    spend(work, scope.marks.len() * corners.len())?;
    let [min_x, min_y, max_x, max_y] = point_bounds(&corners);
    let [dx, dy] = matrix.margin(error);
    // @invariant: Expanding the mark tests the outer envelope of the nominal stroke polygon.
    let mut hit = false;
    for mark in scope.marks {
        if !intersects(bounds, mark.rect) {
            continue;
        }
        let [left, bottom, right, top] = mark.rect.map(|coord| coord * POINT);
        let expanded = [left - dx, bottom - dy, right + dx, top + dy];
        let [l, b, r, t] = expanded;
        if segment {
            let [a, b] = [corners[0], corners[1]];
            if segment_hits(a, b, [left, bottom, right, top], false) {
                hit = true;
            } else if segment_hits(a, b, expanded, true) {
                return Err(Unsupported);
            }
            continue;
        }
        let overlaps_axes = if stroke {
            min_x <= r && l <= max_x && min_y <= t && b <= max_y
        } else {
            intersects([min_x, min_y, max_x, max_y], expanded)
        };
        if overlaps_axes {
            if !convex {
                return Err(Unsupported);
            }
            let orientation = side(corners[0], corners[1], corners[2]).signum();
            hit |= edge_pairs(&corners).all(|(start, end)| {
                [[l, b], [r, b], [r, t], [l, t]].into_iter().any(|point| {
                    let signed = side(start, end, point) * orientation;
                    signed > 0 || (stroke && signed == 0)
                })
            });
        }
    }
    Ok(hit)
}

fn segment_hits([x, y]: Point, [u, v]: Point, [l, b, r, t]: Rect, inclusive: bool) -> bool {
    let sides = [[l, b], [r, b], [r, t], [l, t]].map(|p| side([x, y], [u, v], p));
    if inclusive {
        l <= x.max(u)
            && x.min(u) <= r
            && b <= y.max(v)
            && y.min(v) <= t
            && sides.iter().any(|&s| s >= 0)
            && sides.iter().any(|&s| s <= 0)
    } else {
        intersects([x.min(u), y.min(v), x.max(u), y.max(v)], [l, b, r, t])
            && sides.iter().any(|&s| s > 0)
            && sides.iter().any(|&s| s < 0)
    }
}

fn point_bounds(points: &[Point]) -> Rect {
    let [x, y] = points[0];
    points.iter().fold([x, y, x, y], |[l, b, r, t], &[x, y]| {
        [l.min(x), b.min(y), r.max(x), t.max(y)]
    })
}

fn rounded_ratio(num: i128, den: i128) -> Result<i64> {
    let sign = den.signum();
    let den = den.abs();
    i64::try_from((num * sign + den / 2).div_euclid(den)).map_err(|_| Unsupported)
}

fn dot([x, y]: Point, [u, v]: Point) -> i128 {
    i128::from(x) * i128::from(u) + i128::from(y) * i128::from(v)
}

fn stroke_normal([x, y]: Point, width: i64) -> (Point, Point) {
    let denominator = 4 * dot([x, y], [x, y]);
    let normal = [-y, x].map(|component| {
        let numerator = i128::from(component) * i128::from(width);
        let square = numerator * numerator;
        let floor = (square / denominator).isqrt();
        let twice = 2 * floor + 1;
        let round_up = 4 * square >= denominator * twice * twice;
        let rounded = (floor + i128::from(round_up)) * numerator.signum();
        (
            rounded as i64,
            i64::from(square != denominator * floor * floor),
        )
    });
    ([normal[0].0, normal[1].0], [normal[0].1, normal[1].1])
}

fn miter_tip(
    incoming: Point,
    outgoing: Point,
    normals: Point,
    error: Point,
) -> Result<Option<(Point, Point)>> {
    let projection = dot(incoming, outgoing);
    let squared_lengths = dot(incoming, incoming) * dot(outgoing, outgoing);
    // @invariant: The default miter limit of 10 corresponds to a turn cosine of -49/50.
    if projection < 0 {
        let square = projection * projection;
        let threshold = (squared_lengths / 2500) * 2401 + (squared_lengths % 2500) * 2401 / 2500;
        if square > threshold {
            return Ok(None);
        }
    }
    let low = squared_lengths.isqrt();
    let high = low + i128::from(low * low != squared_lengths);
    if low + projection <= 0 {
        return Err(Unsupported);
    }
    let mut tip = [0; 2];
    let mut margin = [0; 2];
    for axis in 0..2 {
        let sum = i128::from(normals[axis]);
        let error = i128::from(error[axis]);
        // @invariant: The tip offset is the sum of the outer normals divided by 1 + cos(turn).
        let limits = [low, high].map(|length| {
            let divisor = length + projection;
            [
                ((sum - error) * length).div_euclid(divisor),
                -(-(sum + error) * length).div_euclid(divisor),
            ]
        });
        tip[axis] = rounded_ratio(sum * low, low + projection)?;
        let at = i128::from(tip[axis]);
        let lower = limits[0][0].min(limits[1][0]);
        let upper = limits[0][1].max(limits[1][1]);
        margin[axis] = i64::try_from((at - lower).max(upper - at)).map_err(|_| Unsupported)?;
    }
    Ok(Some((tip, margin)))
}

fn stroke_hits(
    points: &[Point],
    closed: bool,
    graphics: Graphics,
    scope: Scope<'_, '_>,
    work: &mut usize,
) -> Result<bool> {
    let count = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let mut normals = Vec::with_capacity(count);
    let mut remove = false;
    for (a, b) in edge_pairs(points).take(count) {
        let (normal, error) = stroke_normal(difference(b, a), graphics.width);
        normals.push((normal, error));
        let contour = [
            translated(a, normal),
            translated(b, normal),
            difference(b, normal),
            difference(a, normal),
        ];
        // @invariant: A collapsed contour keeps the exact centerline and its rounding envelope.
        let outline = if normal == [0; 2] {
            &contour[..2]
        } else {
            &contour[..]
        };
        remove |= polygon_hits(outline, error, graphics.matrix, scope, work, true)?;
    }

    let joins = if closed { 0..points.len() } else { 1..count };
    for index in joins {
        let previous = (index + points.len() - 1) % points.len();
        let center = points[index];
        let incoming = difference(center, points[previous]);
        let outgoing = difference(points[(index + 1) % points.len()], center);
        let turn = cross(incoming, outgoing);
        if turn == 0 {
            if dot(incoming, outgoing) <= 0 {
                return Err(Unsupported);
            }
            continue;
        }

        let outside = -(turn.signum() as i64);
        let [(a, ae), (b, be)] = [normals[previous], normals[index]];
        let [a, b] = [a, b].map(|normal| normal.map(|n| n * outside));
        let mut error = [ae[0].max(be[0]), ae[1].max(be[1])];
        let mut join = vec![center, translated(center, a)];
        if let Some((tip, margin)) =
            miter_tip(incoming, outgoing, translated(a, b), translated(ae, be))?
        {
            join.push(translated(center, tip));
            error = [error[0].max(margin[0]), error[1].max(margin[1])];
        }
        join.push(translated(center, b));
        remove |= polygon_hits(&join, error, graphics.matrix, scope, work, true)?;
    }
    Ok(remove)
}

struct TextBlock {
    start: usize,
    output: usize,
    colors: Vec<u8>,
    font_set: bool,
}

struct PageEdit {
    resources: UsedResources,
    notes: Vec<Object>,
}

struct Edits {
    streams: BTreeMap<Object, Vec<u8>>,
    forms: BTreeMap<Object, UsedResources>,
    pages: BTreeMap<Object, PageEdit>,
    sensitive_content: BTreeSet<Digest>,
    sensitive_images: BTreeSet<Digest>,
}

struct Scan<'a> {
    graph: &'a NativePdfGraph,
    text: RedactionText<'a>,
    edits: Edits,
    content_aliases: BTreeMap<Digest, Digest>,
    image_aliases: BTreeMap<Digest, Digest>,
    note_uses: BTreeMap<Object, bool>,
    active_forms: BTreeSet<Object>,
    work: usize,
    memory: usize,
}

impl<'a> Scan<'a> {
    fn new(inventory: &Inventory<'a>) -> Self {
        let graph = inventory.graph;
        Self {
            graph,
            text: RedactionText::new(graph),
            edits: Edits {
                streams: BTreeMap::new(),
                forms: BTreeMap::new(),
                pages: BTreeMap::new(),
                sensitive_content: BTreeSet::new(),
                sensitive_images: BTreeSet::new(),
            },
            content_aliases: BTreeMap::new(),
            image_aliases: BTreeMap::new(),
            note_uses: BTreeMap::new(),
            active_forms: BTreeSet::new(),
            work: inventory.work,
            memory: graph.remaining_retained() / 2,
        }
    }

    fn page(&mut self, id: Object, page_id: u64, marks: &[NativeRedactionMark]) -> Result<()> {
        let graph = self.graph;
        let page = page(graph, id, &mut self.work)?;
        spend(&mut self.work, marks.len())?;
        let page_marks: Vec<_> = marks
            .iter()
            .filter(|mark| mark.page_id == page_id)
            .collect();
        if page_marks.iter().any(|mark| {
            mark.geometry != page.geometry
                || !valid_rect(mark.rect)
                || !contains(page.geometry.crop_box, mark.rect)
        }) {
            return Err(Unsupported);
        }
        spend(&mut self.memory, 512)?;
        let mut used = UsedResources::new();
        let mut notes = Vec::new();
        let node = dict(graph, id)?;
        let mut graphics = Graphics {
            matrix: Matrix::IDENTITY,
            width: POINT,
        };
        let scope = Scope {
            resources: page.resources,
            clip: page.geometry.crop_box,
            marks: &page_marks,
        };
        let mut saved = Vec::new();
        if let Some(contents) = node.get("Contents") {
            let list = resolve(graph, contents)?
                .as_array()
                .unwrap_or(std::slice::from_ref(contents));
            let mut previous = &[][..];
            for item in list {
                let id = reference(item)?;
                let data = &stream(graph, id)?.data;
                if !separated_content_streams(previous, data) {
                    return Err(Unsupported);
                }
                graphics = self.content(id, graphics, &mut saved, scope, &mut used)?;
                previous = data;
            }
        }
        if !saved.is_empty() {
            return Err(Unsupported);
        }
        if let Some(list) = node.get("Annots") {
            for item in resolve(graph, list)?.as_array().ok_or(Malformed)? {
                spend(&mut self.memory, 128)?;
                let note = reference(item)?;
                let entries = dict(graph, note)?;
                if entries
                    .get("P")
                    .map(reference)
                    .transpose()?
                    .is_some_and(|page| page != id)
                {
                    return Err(Malformed);
                }
                let remove = scope.remove(rect(get(entries, "Rect")?)?, &mut self.work)?;
                same_use(&mut self.note_uses, note, remove)?;
                if !remove {
                    notes.push(note);
                }
            }
        }
        self.edits.pages.insert(
            id,
            PageEdit {
                resources: used,
                notes,
            },
        );
        Ok(())
    }

    fn content(
        &mut self,
        id: Object,
        mut graphics: Graphics,
        saved: &mut Vec<Graphics>,
        scope: Scope<'a, '_>,
        used: &mut UsedResources,
    ) -> Result<Graphics> {
        let source = &stream(self.graph, id)?.data;
        if source.len() > MAX_STREAM {
            return Err(Budget);
        }
        spend(&mut self.work, source.len() + 1)?;
        spend(&mut self.memory, source.len() * 6 + 512)?;
        let mut cursor = ContentCursor::new(
            source,
            PdfReaderLimits {
                max_content_tokens: 16_384,
                max_nesting: 2,
                max_array_items: 4096,
                max_string_bytes: 8192,
                ..Default::default()
            },
        );
        let mut output = Vec::new();
        let mut removals = blake3::Hasher::new();
        let mut removed = false;
        let mut args = Vec::new();
        let mut path: Option<Path> = None;
        let mut text: Option<TextBlock> = None;
        while let Some((token, offset)) = cursor.next().map_err(|_| Unsupported)? {
            spend(&mut self.work, 1)?;
            let end = cursor.position();
            let ContentToken::Word(op) = token else {
                if args.len() == 8 {
                    return Err(Unsupported);
                }
                args.push((token, offset, &source[offset..end]));
                continue;
            };
            let start = args.first().map_or(offset, |(_, start, _)| *start);
            let op = op.as_str();
            let mut emit = true;
            if let Some(block) = &mut text {
                match op {
                    "ET" if args.is_empty() => {
                        let measurement = self.text.measure(
                            scope.resources,
                            &source[block.start..end],
                            &mut self.work,
                        )?;
                        let drop_text = if let Some((font, bounds)) = measurement {
                            let remove =
                                scope.remove(graphics.matrix.bounds(bounds)?, &mut self.work)?;
                            if remove {
                                record_removal(&mut removals, block.start, end);
                                removed = true;
                            } else {
                                self.use_resource(scope.resources, used, "Font", &font)?;
                            }
                            remove
                        } else {
                            true
                        };
                        if drop_text {
                            output.truncate(block.output);
                            output.extend_from_slice(&block.colors);
                            emit = false;
                        }
                        text = None;
                    }
                    "Tf" if !block.font_set => block.font_set = true,
                    "g" | "G" | "rg" | "RG" | "k" | "K" => {
                        color(op, &args)?;
                        emit_operation(&mut block.colors, op, &args)?;
                    }
                    "Td" | "Tm" | "Tj" | "TJ" | "Tr" | "Tc" | "Tw" | "Ts" | "Tz" => {}
                    _ => return Err(Unsupported),
                }
            } else if let Some(building) = &mut path {
                match op {
                    "l" if !building.closed && building.points.len() < 64 => {
                        building.points.push(numbers(&args)?);
                    }
                    "h" if !building.closed && args.is_empty() => building.closed = true,
                    "f" | "F" | "f*" | "S" | "s" | "B" | "B*" | "b" | "b*" if args.is_empty() => {
                        let paint = path.take().ok_or(Malformed)?;
                        let (paint_start, paint_output) = (paint.start, paint.output);
                        if paint.should_remove(op, graphics, scope, &mut self.work)? {
                            output.truncate(paint_output);
                            record_removal(&mut removals, paint_start, end);
                            removed = true;
                            emit = false;
                        }
                    }
                    _ => return Err(Unsupported),
                }
            } else {
                match op {
                    "BT" if args.is_empty() => {
                        text = Some(TextBlock {
                            start,
                            output: output.len(),
                            colors: Vec::new(),
                            font_set: false,
                        })
                    }
                    "m" => {
                        path = Some(Path {
                            start,
                            output: output.len(),
                            points: vec![numbers(&args)?],
                            closed: false,
                        })
                    }
                    "re" => {
                        let [x, y, w, h] = numbers(&args)?;
                        if w == 0 || h == 0 {
                            return Err(Unsupported);
                        }
                        path = Some(Path {
                            start,
                            output: output.len(),
                            closed: true,
                            points: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
                        });
                    }
                    "n" if args.is_empty() => {}
                    "q" if args.is_empty() && saved.len() < 32 => saved.push(graphics),
                    "Q" if args.is_empty() => graphics = saved.pop().ok_or(Unsupported)?,
                    "cm" => {
                        graphics.matrix = graphics.matrix.then(Matrix::new(numbers(&args)?)?)?
                    }
                    "w" => {
                        let [width] = numbers(&args)?;
                        if !(1..=1000 * POINT).contains(&width) {
                            return Err(Unsupported);
                        }
                        graphics.width = width;
                    }
                    "g" | "G" | "rg" | "RG" | "k" | "K" => color(op, &args)?,
                    "Do" => {
                        let [(ContentToken::Name(name), _, _)] = args.as_slice() else {
                            return Err(Unsupported);
                        };
                        let target = self.use_resource(scope.resources, used, "XObject", name)?;
                        self.invoke(target, graphics, scope)?;
                    }
                    _ => return Err(Unsupported),
                }
            }
            if emit {
                emit_operation(&mut output, op, &args)?;
            }
            args.clear();
        }
        if !args.is_empty() || path.is_some() || text.is_some() {
            return Err(Unsupported);
        }
        let digest = *blake3::hash(source).as_bytes();
        same_use(
            &mut self.content_aliases,
            digest,
            *removals.finalize().as_bytes(),
        )?;
        if removed {
            self.edits.sensitive_content.insert(digest);
        }
        same_use(&mut self.edits.streams, id, output)?;
        Ok(graphics)
    }

    fn use_resource(
        &mut self,
        resources: Option<&Dict>,
        used: &mut UsedResources,
        group: &'static str,
        name: &str,
    ) -> Result<Object> {
        spend(&mut self.memory, 256 + name.len())?;
        let id = resolve_resource(self.graph, resources, group, name)?;
        same_use(used, (group, name.into()), id)?;
        Ok(id)
    }

    fn invoke(&mut self, id: Object, graphics: Graphics, scope: Scope<'a, '_>) -> Result<()> {
        let object = stream(self.graph, id)?;
        if name(&object.dict, "Subtype") == Some("Image") {
            let image = Image::read(object)?;
            let bounds = graphics.matrix.bounds([0, 0, POINT, POINT])?;
            if !contains(scope.clip, bounds) {
                return Err(Unsupported);
            }
            spend(&mut self.memory, object.data.len() + 512)?;
            spend(
                &mut self.work,
                scope.marks.len() + object.data.len().div_ceil(16) + 1,
            )?;
            let mut data = object.data.clone();
            for mark in scope.marks {
                image.scrub(&mut data, graphics.matrix, mark.rect, &mut self.work)?;
            }
            let before = *blake3::hash(&object.data).as_bytes();
            let after = *blake3::hash(&data).as_bytes();
            same_use(&mut self.image_aliases, before, after)?;
            if before != after {
                self.edits.sensitive_images.insert(before);
            }
            return same_use(&mut self.edits.streams, id, data);
        }
        if self.active_forms.len() >= MAX_FORM_DEPTH || !self.active_forms.insert(id) {
            return Err(Unsupported);
        }
        let (matrix, bounds) = form_geometry(&object.dict)?;
        let matrix = graphics.matrix.then(matrix)?;
        let [l, b, r, t] = matrix.clip_bounds(bounds)?;
        let clip = [
            scope.clip[0].max(l),
            scope.clip[1].max(b),
            scope.clip[2].min(r),
            scope.clip[3].min(t),
        ];
        if !valid_rect(clip) {
            return Err(Unsupported);
        }
        let resources = match object.dict.get("Resources") {
            Some(value) => Some(dictionary(resolve(self.graph, value)?)?),
            None => scope.resources,
        };
        let mut used = UsedResources::new();
        let mut saved = Vec::new();
        self.content(
            id,
            Graphics { matrix, ..graphics },
            &mut saved,
            Scope {
                resources,
                clip,
                ..scope
            },
            &mut used,
        )?;
        if !saved.is_empty() {
            return Err(Unsupported);
        }
        self.active_forms.remove(&id);
        same_use(&mut self.edits.forms, id, used)
    }
}

#[derive(Clone, Copy)]
struct Scope<'a, 'b> {
    resources: Option<&'a Dict>,
    clip: Rect,
    marks: &'b [&'b NativeRedactionMark],
}

impl Scope<'_, '_> {
    fn remove(self, bounds: Rect, work: &mut usize) -> Result<bool> {
        if !contains(self.clip, bounds) {
            return Err(Unsupported);
        }
        spend(work, self.marks.len())?;
        Ok(self.marks.iter().any(|mark| intersects(mark.rect, bounds)))
    }
}

fn same_use<K: Ord, V: PartialEq>(map: &mut BTreeMap<K, V>, key: K, value: V) -> Result<()> {
    match map.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(value);
        }
        Entry::Occupied(entry) if entry.get() == &value => {}
        Entry::Occupied(_) => return Err(Unsupported),
    }
    Ok(())
}

fn record_removal(hasher: &mut blake3::Hasher, start: usize, end: usize) {
    hasher.update(&(start as u64).to_le_bytes());
    hasher.update(&(end as u64).to_le_bytes());
}

type Operand<'a> = (ContentToken, usize, &'a [u8]);

fn numbers<const N: usize>(args: &[Operand<'_>]) -> Result<[i64; N]> {
    if args.len() != N {
        return Err(Unsupported);
    }
    let mut out = [0; N];
    for (dst, (token, _, raw)) in out.iter_mut().zip(args) {
        if !matches!(token, ContentToken::Integer(_) | ContentToken::Number(_)) {
            return Err(Unsupported);
        }
        *dst = redaction_number(raw).ok_or(Unsupported)?;
    }
    Ok(out)
}

fn color(op: &str, args: &[Operand<'_>]) -> Result<()> {
    let channels = match op {
        "g" | "G" => 1,
        "rg" | "RG" => 3,
        _ => 4,
    };
    if args.len() != channels
        || args.iter().any(|(token, _, raw)| {
            !matches!(token, ContentToken::Number(_) | ContentToken::Integer(_)) || raw.len() > 16
        })
    {
        return Err(Unsupported);
    }
    Ok(())
}

fn hex_string(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(b'<');
    for &byte in bytes {
        out.extend([HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]]);
    }
    out.push(b'>');
}

fn emit_operation(out: &mut Vec<u8>, op: &str, args: &[Operand<'_>]) -> Result<()> {
    for (token, _, raw) in args {
        match token {
            ContentToken::String(bytes) if op == "Tj" => hex_string(out, bytes),
            ContentToken::Array(items) if op == "TJ" => {
                out.push(b'[');
                for item in items {
                    match item {
                        ContentToken::Integer(0) => out.extend(b"0 "),
                        ContentToken::String(bytes) => hex_string(out, bytes),
                        _ => return Err(Unsupported),
                    }
                }
                out.push(b']');
            }
            ContentToken::Name(_) | ContentToken::Integer(_) | ContentToken::Number(_) => {
                out.extend_from_slice(raw)
            }
            _ => return Err(Unsupported),
        }
        out.push(b' ');
    }
    out.extend_from_slice(op.as_bytes());
    out.push(b'\n');
    Ok(())
}

pub(super) fn scrub(
    graph: &mut NativePdfGraph,
    order: &[u64],
    marks: &[NativeRedactionMark],
) -> Result<()> {
    if marks.is_empty()
        || marks.len() > MAX_MARKS
        || marks.iter().any(|mark| !order.contains(&mark.page_id))
    {
        return Err(Unsupported);
    }
    let (roles, edits, remaining_work) = scan_document(graph, order, marks)?;
    graph.charge_page_work(graph.remaining_work() - remaining_work)?;
    apply(graph, roles, edits)
}

pub(super) fn check_input_budget(graph: &NativePdfGraph, origin_len: Option<usize>) -> Result<()> {
    if origin_len.is_some_and(|len| len > MAX_INPUT)
        || graph.original_bytes().len() > graph.limits.max_source_bytes.min(MAX_INPUT)
    {
        return Err(Budget);
    }
    let pages = graph.pages.len();
    (pages <= MAX_PAGES
        && verify_raster::within_pixel_budget((0..pages).map(|index| geometry(graph, index))))
    .then_some(())
    .ok_or(Budget)
}

pub(super) fn supports_document(graph: &NativePdfGraph, order: &[u64]) -> bool {
    scan_document(graph, order, &[]).is_ok()
}

fn scan_document(
    graph: &NativePdfGraph,
    order: &[u64],
    marks: &[NativeRedactionMark],
) -> Result<(BTreeMap<Object, Role>, Edits, usize)> {
    if order.len() != graph.pages.len() || order.is_empty() || order.len() > MAX_PAGES {
        return Err(Unsupported);
    }
    let inventory = Inventory::read(graph)?;
    let mut scan = Scan::new(&inventory);
    for (&page, &page_id) in graph.pages.iter().zip(order) {
        scan.page(page, page_id, marks)?;
    }
    for (&id, &role) in &inventory.roles {
        if role == Role::XObject {
            spend(&mut scan.work, 1)?;
            if name(&stream(graph, id)?.dict, "Subtype") == Some("Form")
                && !scan.edits.forms.contains_key(&id)
            {
                return Err(Unsupported);
            }
        }
    }
    Ok((inventory.roles, scan.edits, scan.work))
}

fn apply(
    graph: &mut NativePdfGraph,
    roles: BTreeMap<Object, Role>,
    mut edits: Edits,
) -> Result<()> {
    graph.trailer.remove("Info");
    graph.trailer.remove("ID");
    for (&id, &role) in &roles {
        match (role, graph.objects.get_mut(&id).ok_or(Malformed)?) {
            (Role::Catalog, PdfValue::Dictionary(dict)) => {
                dict.remove("Metadata");
                dict.remove("FlowPDFSource");
            }
            (Role::Tree, PdfValue::Dictionary(dict)) => {
                dict.remove("Resources");
            }
            (Role::Page, PdfValue::Dictionary(dict)) => {
                let page = edits.pages.remove(&id).ok_or(Malformed)?;
                dict.remove("Metadata");
                dict.remove("Annots");
                dict.remove("Resources");
                if !page.notes.is_empty() {
                    dict.insert(
                        "Annots".into(),
                        PdfValue::Array(page.notes.into_iter().map(PdfValue::Reference).collect()),
                    );
                }
                if !page.resources.is_empty() {
                    dict.insert(
                        "Resources".into(),
                        PdfValue::Dictionary(used_resources(page.resources)),
                    );
                }
            }
            (Role::XObject, PdfValue::Stream(stream))
                if name(&stream.dict, "Subtype") == Some("Form") =>
            {
                if let Some(resources) = edits.forms.remove(&id) {
                    stream.dict.insert(
                        "Resources".into(),
                        PdfValue::Dictionary(used_resources(resources)),
                    );
                }
            }
            _ => {}
        }
    }
    graph.refresh_page_graph(&BTreeSet::new())?;
    let mut streams = Vec::new();
    for (&id, value) in &graph.objects {
        if !matches!(value, PdfValue::Stream(_)) {
            continue;
        }
        match roles[&id] {
            Role::Metadata | Role::Source => return Err(Unsupported),
            Role::Content | Role::XObject if !edits.streams.contains_key(&id) => {
                return Err(Unsupported);
            }
            _ => streams.push(id),
        }
    }
    for id in streams {
        let replacement = edits.streams.remove(&id);
        let before = stream(graph, id)?;
        let bytes = replacement.as_deref().unwrap_or(&before.data);
        let digest = *blake3::hash(bytes).as_bytes();
        let forbidden = if name(&before.dict, "Subtype") == Some("Image") {
            edits.sensitive_images.contains(&digest)
        } else {
            edits.sensitive_content.contains(&digest)
        };
        if forbidden {
            return Err(Unsupported);
        }
        let size = bytes.len();
        graph.reserve_replacement_stream(size)?;
        graph.charge_page_work(size.div_ceil(16) + 1)?;
        let Some(PdfValue::Stream(stream)) = graph.objects.get_mut(&id) else {
            return Err(Malformed);
        };
        if let Some(replacement) = replacement {
            stream.data = replacement;
        }
        stream.dict.remove("Filter");
        stream.encoded = if size >= 1024 {
            let compressed = compress_to_vec_zlib(&stream.data, 6);
            if compressed.len() < size {
                stream
                    .dict
                    .insert("Filter".into(), PdfValue::Name("FlateDecode".into()));
                compressed
            } else {
                stream.data.clone()
            }
        } else {
            stream.data.clone()
        };
        stream.dict.insert(
            "Length".into(),
            PdfValue::Integer(stream.encoded.len() as i64),
        );
    }
    graph.refresh_page_graph(&BTreeSet::new())
}

#[cfg(test)]
mod tests {
    use super::super::{
        NativeEditLimits, NativePageCommand, NativePageSide, NativePageTarget, NativePdfSession,
        tests::redaction_fixture, verifier::NativeVerificationFailure::UnsupportedProfile,
        verify_text, writer,
    };
    use super::*;

    fn page_stream_pair(first: &str, second: &str) -> Vec<u8> {
        let mut graph =
            NativePdfGraph::open(&redaction_fixture(first), NativeEditLimits::default()).unwrap();
        let Some(PdfValue::Dictionary(page)) = graph.objects.get_mut(&graph.pages[0]) else {
            unreachable!()
        };
        let first = reference(get(page, "Contents").unwrap()).unwrap();
        let next = Object {
            object_number: first.object_number + 1,
            generation: 0,
        };
        page.insert(
            "Contents".into(),
            PdfValue::Array(vec![PdfValue::Reference(first), PdfValue::Reference(next)]),
        );
        graph.objects.insert(
            next,
            PdfValue::Stream(PdfStream {
                dict: BTreeMap::from([("Length".into(), PdfValue::Integer(second.len() as i64))]),
                data: second.as_bytes().to_vec(),
                encoded: second.as_bytes().to_vec(),
                source_offset: 0,
            }),
        );
        writer::rewrite(&graph).unwrap()
    }

    #[test]
    fn page_stream_boundaries_require_separation_with_an_empty_graphics_stack() {
        let order = [1];
        let marks = [NativeRedactionMark {
            page_id: 1,
            geometry: NativeRedactionGeometry {
                media_box: [0, 0, 100_000, 100_000],
                crop_box: [0, 0, 100_000, 100_000],
                rotation: 0,
            },
            rect: [9_000, 9_000, 12_000, 12_000],
        }];
        let retained = "55 55 10 10 re f";
        let accepted = page_stream_pair("10 10 10 10 re f\n", retained);
        let limits = NativeEditLimits::default();
        let mut graph = NativePdfGraph::open(&accepted, limits).unwrap();
        scrub(&mut graph, &order, &marks).unwrap();
        let candidate = writer::rewrite(&graph).unwrap();
        assert!(verify_text::verify(&accepted, &candidate, &order, &marks).is_ok());
        assert!(verify_raster::verify(&accepted, &candidate, &order, &marks).is_ok());

        let unproven_accepted = page_stream_pair("10 10 10 10 re f ", retained);
        let unproven_candidate = page_stream_pair("0 g ", retained);
        let mut graph = NativePdfGraph::open(&unproven_accepted, limits).unwrap();
        assert!(matches!(
            scrub(&mut graph, &order, &marks),
            Err(Unsupported)
        ));
        for (before, after) in [
            (&unproven_accepted, &candidate),
            (&accepted, &unproven_candidate),
        ] {
            assert!(matches!(
                verify_text::verify(before, after, &order, &marks),
                Err(UnsupportedProfile)
            ));
            assert!(matches!(
                verify_raster::verify(before, after, &order, &marks),
                Err(UnsupportedProfile)
            ));
        }
    }

    #[test]
    fn transform_rounds_paint_outward_and_clips_inward() {
        let matrix = Matrix::new([0, 501, -501, 0, 0, 0]).unwrap();
        let local = [1, 1, 5, 5];
        assert_eq!(matrix.bounds(local), Ok([-3, 0, 0, 3]));
        assert_eq!(matrix.clip_bounds(local), Ok([-2, 1, -1, 2]));
    }

    #[test]
    fn exact_right_angle_miter_and_default_bevel_limit() {
        assert_eq!(
            miter_tip([8_000, 0], [0, 8_000], [1_000, -1_000], [0, 0]),
            Ok(Some(([1_000, -1_000], [0, 0]))),
        );
        assert_eq!(
            miter_tip([8_000, 0], [-8_000, 500], [125, 0], [1, 1]),
            Ok(None),
        );
    }

    fn session_with_crop_width(width: i64) -> NativePdfSession {
        let limits = NativeEditLimits::default();
        let mut graph =
            NativePdfGraph::open(&redaction_fixture("10 10 10 10 re f"), limits).unwrap();
        let Some(PdfValue::Dictionary(page)) = graph.objects.get_mut(&graph.pages[0]) else {
            unreachable!()
        };
        page.insert(
            "MediaBox".into(),
            PdfValue::Array([0, 0, 3_000, 3_000].map(PdfValue::Integer).to_vec()),
        );
        page.insert(
            "CropBox".into(),
            PdfValue::Array([0, 0, width, 2_000].map(PdfValue::Integer).to_vec()),
        );
        page.insert("Rotate".into(), PdfValue::Integer(90));
        NativePdfSession::open_scoped(&writer::rewrite(&graph).unwrap(), limits, [5; 16]).unwrap()
    }

    #[test]
    fn redaction_admission_checks_page_and_document_raster_limits() {
        assert!(!session_with_crop_width(2_001).redaction_admitted());

        let mut session = session_with_crop_width(2_000);
        let target = NativePageTarget {
            page: 0,
            stable_id: session.snapshot().page_state.order[0],
        };
        for request_id in 1..=3 {
            assert!(session.redaction_admitted(), "{request_id} pages");
            session
                .apply_page(
                    session.snapshot().binding,
                    request_id,
                    &NativePageCommand::InsertBlankPage {
                        target,
                        side: NativePageSide::After,
                    },
                )
                .unwrap();
        }
        assert_eq!(session.snapshot().page_state.order.len(), 4);
        assert!(!session.redaction_admitted());
        session
            .apply_page(
                session.snapshot().binding,
                4,
                &NativePageCommand::DeletePage {
                    target: NativePageTarget {
                        page: 1,
                        stable_id: session.snapshot().page_state.order[1],
                    },
                },
            )
            .unwrap();
        assert!(session.redaction_admitted());
    }

    #[test]
    fn ordinary_admission_and_scrub_inventory_have_distinct_boundaries() {
        let pdf = redaction_fixture("10 10 10 10 re f");
        let mut graph = NativePdfGraph::open(&pdf, NativeEditLimits::default()).unwrap();
        assert!(Inventory::read(&graph).is_ok());
        let root = reference(get(&graph.trailer, "Root").unwrap()).unwrap();
        let page = graph.pages[0];
        for (id, fields) in [
            (
                root,
                &[
                    "Names",
                    "AcroForm",
                    "Perms",
                    "OpenAction",
                    "StructTreeRoot",
                    "OCProperties",
                    "AF",
                ][..],
            ),
            (page, &["PieceInfo", "AA", "Thumb", "Group"][..]),
        ] {
            for &field in fields {
                let Some(PdfValue::Dictionary(dict)) = graph.objects.get_mut(&id) else {
                    unreachable!()
                };
                dict.insert(field.into(), PdfValue::Null);
                assert!(
                    matches!(Inventory::read(&graph), Err(Unsupported)),
                    "{field}"
                );
                let Some(PdfValue::Dictionary(dict)) = graph.objects.get_mut(&id) else {
                    unreachable!()
                };
                dict.remove(field);
            }
        }
        for field in ["Prev", "Encrypt", "XRefStm"] {
            graph.trailer.insert(field.into(), PdfValue::Null);
            assert!(
                matches!(Inventory::read(&graph), Err(Unsupported)),
                "{field}"
            );
            graph.trailer.remove(field);
        }

        let mut image = PdfStream {
            dict: BTreeMap::from([
                ("Subtype".into(), PdfValue::Name("Image".into())),
                ("Width".into(), PdfValue::Integer(1)),
                ("Height".into(), PdfValue::Integer(1)),
                ("ColorSpace".into(), PdfValue::Name("DeviceGray".into())),
                ("BitsPerComponent".into(), PdfValue::Integer(8)),
                ("Length".into(), PdfValue::Integer(1)),
            ]),
            data: vec![127],
            encoded: vec![127],
            source_offset: 0,
        };
        let mut inventory = Inventory::read(&graph).unwrap();
        assert!(inventory.inspect_stream(&image, Role::XObject).is_ok());
        for field in [
            "Mask",
            "SMask",
            "Alternates",
            "Decode",
            "DecodeParms",
            "Metadata",
        ] {
            image.dict.insert(field.into(), PdfValue::Null);
            assert!(
                matches!(
                    inventory.inspect_stream(&image, Role::XObject),
                    Err(Unsupported)
                ),
                "{field}"
            );
            image.dict.remove(field);
        }
        assert_eq!(graph.original_bytes(), pdf);
    }
}
