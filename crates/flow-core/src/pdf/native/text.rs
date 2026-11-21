use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use serde::{Deserialize, Serialize};
use ttf_parser::{Face, GlyphId, Rect};

use super::super::{
    reader::{PdfObjectRef, PdfReaderLimits, PdfStream, PdfValue},
    scene::{ContentCursor, ContentToken, NativePdfNotePage},
};
use super::graph::{NativeEditError, NativePdfGraph};
use super::verifier::{Reservation, Workspace};
use super::{NativeBinding, NativePageId, NativeSessionError};
use NativeSessionError::Budget;

pub const NATIVE_MAX_TEXT_BYTES: usize = 16 * 1024;
const SCALE: i64 = 1_000;
const MAX_COORDINATE: i64 = 1_000_000 * SCALE;
const MAX_MAP_ENTRIES: usize = 4_096;
const MAX_ISLANDS_PER_PAGE: usize = 128;
const MAX_CONTENT_BYTES: usize = 256 * 1024;
const MAX_CMAP_BYTES: usize = 128 * 1024;
const MAX_TOKENS: usize = 16_384;

type Dict = BTreeMap<String, PdfValue>;
type Bounds = [i64; 4];
type TextResult<T> = Result<T, NativeTextRefusal>;
type RecognitionResult<T> = Result<T, NativeSessionError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NativeTextCommand {
    ReplaceText { island: [u8; 32], text: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NativeTextRefusal {
    #[serde(rename = "FLOW_PDF_NATIVE_TEXT_FONT")]
    Font,
    #[serde(rename = "FLOW_PDF_NATIVE_TEXT_MAPPING")]
    Mapping,
    #[serde(rename = "FLOW_PDF_NATIVE_TEXT_LAYOUT")]
    Layout,
    #[serde(rename = "FLOW_PDF_NATIVE_TEXT_OWNERSHIP")]
    Ownership,
    #[serde(rename = "FLOW_PDF_NATIVE_TEXT_OVERFLOW")]
    Overflow,
}

impl NativeTextRefusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Font => "FLOW_PDF_NATIVE_TEXT_FONT",
            Self::Mapping => "FLOW_PDF_NATIVE_TEXT_MAPPING",
            Self::Layout => "FLOW_PDF_NATIVE_TEXT_LAYOUT",
            Self::Ownership => "FLOW_PDF_NATIVE_TEXT_OWNERSHIP",
            Self::Overflow => "FLOW_PDF_NATIVE_TEXT_OVERFLOW",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeTextProfile {
    TrueTypeWinAnsi,
    IdentityHCidFontType2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTextIsland {
    pub id: [u8; 32],
    pub text: String,
    pub bounds: Bounds,
    pub profile: NativeTextProfile,
    pub operator: NativeTextOperator,
    pub max_advance: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NativeTextOperator {
    Tj,
    TJ,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTextPage {
    pub page: NativePageId,
    pub islands: Vec<NativeTextIsland>,
    #[serde(deserialize_with = "required_refusal")]
    pub refusal: Option<NativeTextRefusal>,
}

fn required_refusal<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NativeTextRefusal>, D::Error> {
    Option::deserialize(deserializer)
}

struct Show {
    operand: Range<usize>,
    font: String,
    size: i64,
    origin: [i64; 2],
    codes: Vec<u8>,
    array: bool,
}

struct Island {
    view: NativeTextIsland,
    show: Show,
    metrics: Metrics,
}

struct Content<'a> {
    reference: PdfObjectRef,
    stream: &'a PdfStream,
    resources: &'a Dict,
    copy_on_write: bool,
}

struct RecognizedPage<'a, 'w> {
    content: Content<'a>,
    islands: Vec<Island>,
    text_memory: Reservation<'w>,
}

fn value<'a>(graph: &'a NativePdfGraph, mut value: &'a PdfValue) -> Option<&'a PdfValue> {
    for _ in 0..=graph.limits.max_nesting {
        match value {
            PdfValue::Reference(reference) => value = graph.objects.get(reference)?,
            _ => return Some(value),
        }
    }
    None
}

fn entry<'a>(graph: &'a NativePdfGraph, dict: &'a Dict, name: &str) -> Option<&'a PdfValue> {
    value(graph, dict.get(name)?)
}

fn name<'a>(graph: &'a NativePdfGraph, dict: &'a Dict, key: &str) -> Option<&'a str> {
    entry(graph, dict, key).and_then(PdfValue::as_name)
}

fn stream(value: &PdfValue) -> Option<&PdfStream> {
    match value {
        PdfValue::Stream(stream) => Some(stream),
        _ => None,
    }
}

fn inherited_resources(graph: &NativePdfGraph, mut page: PdfObjectRef) -> Option<&Dict> {
    for _ in 0..=graph.limits.max_nesting {
        let dict = graph.objects.get(&page)?.as_dictionary()?;
        if dict.contains_key("Resources") {
            return entry(graph, dict, "Resources")?.as_dictionary();
        }
        page = dict.get("Parent")?.as_reference()?;
    }
    None
}

fn content<'a>(
    graph: &'a NativePdfGraph,
    page: &NativePdfNotePage,
) -> TextResult<Option<Content<'a>>> {
    use NativeTextRefusal::{Font, Layout, Ownership};
    let dict = graph
        .objects
        .get(&page.object)
        .and_then(PdfValue::as_dictionary)
        .ok_or(Layout)?;
    let Some(contents) = dict.get("Contents") else {
        return Ok(None);
    };
    let operand = match contents {
        PdfValue::Array(values) if values.is_empty() => return Ok(None),
        PdfValue::Array(values) if values.len() == 1 => &values[0],
        PdfValue::Reference(_) => contents,
        _ => return Err(Ownership),
    };
    let reference = operand.as_reference().ok_or(Ownership)?;
    let stream = graph
        .objects
        .get(&reference)
        .and_then(stream)
        .ok_or(Ownership)?;
    if stream.data.is_empty() {
        return Ok(None);
    }
    if graph
        .reverse
        .get(&reference)
        .is_none_or(|owners| owners.len() != 1 || !owners.contains(&page.object))
    {
        return Err(Ownership);
    }
    fn references(value: &PdfValue, reference: PdfObjectRef) -> bool {
        match value {
            PdfValue::Reference(object) => *object == reference,
            PdfValue::Array(items) => items.iter().any(|value| references(value, reference)),
            PdfValue::Dictionary(dict) => dict.values().any(|value| references(value, reference)),
            _ => false,
        }
    }
    let copy_on_write = dict
        .iter()
        .any(|(key, value)| key != "Contents" && references(value, reference))
        || graph
            .trailer
            .values()
            .any(|value| references(value, reference));
    if dict
        .get("Resources")
        .is_some_and(|value| references(value, reference))
    {
        return Err(Ownership);
    }
    if stream.data.len() > MAX_CONTENT_BYTES
        || !local_stream(stream)
        || stream.dict.iter().any(|(key, value)| {
            !matches!(key.as_str(), "Length" | "Filter" | "DecodeParms") || has_reference(value)
        })
    {
        return Err(Layout);
    }
    let resources = inherited_resources(graph, page.object).ok_or(Font)?;
    Ok(Some(Content {
        reference,
        stream,
        resources,
        copy_on_write,
    }))
}

fn local_stream(stream: &PdfStream) -> bool {
    ["F", "FFilter", "FDecodeParms", "DecodeParms"]
        .into_iter()
        .all(|key| matches!(stream.dict.get(key), None | Some(PdfValue::Null)))
}

fn has_reference(value: &PdfValue) -> bool {
    match value {
        PdfValue::Reference(_) => true,
        PdfValue::Array(items) => items.iter().any(has_reference),
        PdfValue::Dictionary(dict) => dict.values().any(has_reference),
        _ => false,
    }
}

fn milli(raw: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(raw).ok()?;
    let negative = text.starts_with('-');
    let magnitude = text.trim_start_matches(['-', '+']);
    let (whole, fraction) = magnitude.split_once('.').unwrap_or((magnitude, ""));
    let whole = if whole.is_empty() {
        0
    } else {
        whole.parse::<i64>().ok()?
    };
    let fraction = fraction.trim_end_matches('0');
    let fractional = if fraction.is_empty() {
        0
    } else {
        let zeros = u32::try_from(3usize.checked_sub(fraction.len())?).ok()?;
        fraction
            .parse::<i64>()
            .ok()?
            .checked_mul(10_i64.pow(zeros))?
    };
    let magnitude = whole.checked_mul(SCALE)?.checked_add(fractional)?;
    (magnitude <= MAX_COORDINATE).then_some(if negative { -magnitude } else { magnitude })
}

fn shows(
    data: &[u8],
    workspace: &Workspace,
    retained: &mut Reservation<'_>,
) -> RecognitionResult<Vec<Show>> {
    use NativeTextRefusal::Layout;
    let _parser = reserve_content_tokens(
        workspace,
        data,
        MAX_TOKENS,
        6 * size_of::<(ContentToken, Range<usize>, Option<i64>)>(),
    )?;
    let limits = PdfReaderLimits {
        max_content_tokens: MAX_TOKENS,
        max_array_items: MAX_TOKENS,
        max_nesting: 2,
        max_string_bytes: NATIVE_MAX_TEXT_BYTES,
        ..Default::default()
    };
    let mut cursor = ContentCursor::new(data, limits);
    let mut result = Vec::new();
    let mut operands = Vec::with_capacity(6);
    let mut font = None;
    let mut origin = [0, 0];
    let mut positioned = false;
    let mut in_text = false;
    let mut shown = false;
    let mut saves = 0usize;
    while let Some((token, start)) = cursor.next().map_err(|_| Layout)? {
        let span = start..cursor.position();
        let ContentToken::Word(op) = token else {
            if operands.len() >= 6 {
                return Err(Layout.into());
            }
            let number = match token {
                ContentToken::Integer(_) | ContentToken::Number(_) => milli(&data[span.clone()]),
                _ => None,
            };
            operands.push((token, span, number));
            continue;
        };
        let numbers = |expected: &[i64]| {
            operands.len() == expected.len()
                && operands
                    .iter()
                    .zip(expected)
                    .all(|((_, _, number), expected)| *number == Some(*expected))
        };
        match op.as_str() {
            "q" if !in_text && operands.is_empty() && saves < 64 => saves += 1,
            "Q" if !in_text && operands.is_empty() && saves > 0 => saves -= 1,
            "BT" if !in_text && operands.is_empty() => {
                in_text = true;
                shown = false;
                positioned = false;
                font = None;
                origin = [0, 0];
            }
            "ET" if in_text && operands.is_empty() => in_text = false,
            "Tf" if in_text && !shown => {
                let [(ContentToken::Name(name), span, _), (_, _, Some(size))] = operands.as_slice()
                else {
                    return Err(Layout.into());
                };
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'#' | b'{' | b'}'))
                    || *size <= 0
                {
                    return Err(Layout.into());
                }
                let raw_name =
                    std::str::from_utf8(&data[span.start + 1..span.end]).map_err(|_| Layout)?;
                font = Some((raw_name, *size));
            }
            "Td" | "Tm" if in_text && !shown && !positioned => {
                let offset = if op == "Tm" {
                    if operands.len() != 6
                        || !operands[..4].iter().map(|(_, _, number)| *number).eq([
                            Some(SCALE),
                            Some(0),
                            Some(0),
                            Some(SCALE),
                        ])
                    {
                        return Err(Layout.into());
                    }
                    4
                } else {
                    0
                };
                let [(_, _, Some(x)), (_, _, Some(y))] = &operands[offset..] else {
                    return Err(Layout.into());
                };
                origin = [*x, *y];
                positioned = true;
            }
            "Tj" | "TJ" if in_text && !shown => {
                let [(token, span, _)] = operands.as_slice() else {
                    return Err(Layout.into());
                };
                let array = op == "TJ";
                let codes = match (array, token) {
                    (false, ContentToken::String(bytes)) => bytes,
                    (true, ContentToken::Array(items)) => {
                        let mut bytes = None;
                        for item in items {
                            match item {
                                ContentToken::String(chunk) => {
                                    if bytes.is_some() || chunk.len() > NATIVE_MAX_TEXT_BYTES {
                                        return Err(Layout.into());
                                    }
                                    bytes = Some(chunk);
                                }
                                ContentToken::Integer(0) => {}
                                _ => return Err(Layout.into()),
                            }
                        }
                        bytes.ok_or(Layout)?
                    }
                    _ => return Err(Layout.into()),
                };
                let (font, size) = font.take().ok_or(Layout)?;
                if codes.is_empty() || result.len() >= MAX_ISLANDS_PER_PAGE {
                    return Err(Layout.into());
                }
                retained
                    .grow(font.len() + codes.len())
                    .map_err(|_| Budget)?;
                retained
                    .push(
                        &mut result,
                        Show {
                            operand: span.clone(),
                            font: font.to_owned(),
                            size,
                            origin,
                            codes: codes.clone(),
                            array,
                        },
                    )
                    .map_err(|_| Budget)?;
                shown = true;
            }
            "Tr" | "Tc" | "Tw" | "Ts" if in_text && numbers(&[0]) => {}
            "Tz" if in_text && numbers(&[100 * SCALE]) => {}
            "cm" if !in_text && numbers(&[SCALE, 0, 0, SCALE, 0, 0]) => {}
            "g" | "G" | "rg" | "RG" | "k" | "K" => {
                let count = match op.as_str() {
                    "g" | "G" => 1,
                    "rg" | "RG" => 3,
                    _ => 4,
                };
                if operands.len() != count || !operands.iter().all(|(_, _, n)| n.is_some()) {
                    return Err(Layout.into());
                }
            }
            "Do" => return Err(NativeTextRefusal::Ownership.into()),
            _ => return Err(Layout.into()),
        }
        operands.clear();
    }
    if in_text || saves != 0 || !operands.is_empty() {
        return Err(Layout.into());
    }
    Ok(result)
}

pub(super) fn reserve_content_tokens<'a>(
    workspace: &'a Workspace,
    input: &[u8],
    max_tokens: usize,
    operand_bytes: usize,
) -> Result<Reservation<'a>, NativeEditError> {
    let array_bytes = 4 * possible_array_items(input, max_tokens) * size_of::<ContentToken>();
    workspace
        .reserve(array_bytes + 8 * input.len() + operand_bytes)
        .map_err(|_| NativeEditError::Budget)
}

fn possible_array_items(input: &[u8], limit: usize) -> usize {
    let mut bytes = input.iter().copied();
    let mut arrays = 0_usize;
    let mut items = 0_usize;
    while let Some(byte) = bytes.next() {
        match byte {
            b'%' => {
                bytes.by_ref().find(|byte| matches!(byte, b'\r' | b'\n'));
            }
            b'(' => {
                let mut parentheses = 1_usize;
                while let Some(byte) = bytes.next() {
                    match byte {
                        b'\\' => {
                            bytes.next();
                        }
                        b'(' => parentheses += 1,
                        b')' => {
                            parentheses -= 1;
                            if parentheses == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                items += usize::from(arrays != 0);
            }
            b'<' => {
                bytes.by_ref().find(|&byte| byte == b'>');
                items += usize::from(arrays != 0);
            }
            b'[' => {
                items += usize::from(arrays != 0);
                arrays += 1;
            }
            b']' => arrays = arrays.saturating_sub(1),
            0 | b'\t' | b'\n' | 12 | b'\r' | b' ' => {}
            _ => items += usize::from(arrays != 0),
        }
        if items >= limit {
            return limit;
        }
    }
    items
}

struct CMap<'a> {
    rest: &'a [u8],
}

impl<'a> CMap<'a> {
    fn next(&mut self) -> Option<&'a [u8]> {
        self.rest = self.rest.trim_ascii_start();
        while self.rest.starts_with(b"%") {
            let end = self
                .rest
                .iter()
                .position(|byte| matches!(byte, b'\r' | b'\n'))?;
            self.rest = self.rest[end..].trim_ascii_start();
        }
        let first = *self.rest.first()?;
        let end = if self.rest.starts_with(b"<<") || self.rest.starts_with(b">>") {
            2
        } else if first == b'<' {
            self.rest.iter().position(|byte| *byte == b'>')? + 1
        } else if first == b'(' {
            self.rest.iter().position(|byte| *byte == b')')? + 1
        } else {
            1 + self.rest[1..]
                .iter()
                .take_while(|byte| !byte.is_ascii_whitespace() && !b"<>/()%[]".contains(byte))
                .count()
        };
        let (token, rest) = self.rest.split_at(end);
        self.rest = rest;
        Some(token)
    }

    fn expect(&mut self, tokens: &[&[u8]]) -> Option<()> {
        tokens
            .iter()
            .all(|token| self.next() == Some(token))
            .then_some(())
    }

    fn number(&mut self) -> Option<usize> {
        decimal_count(self.next()?)
    }

    fn hex(&mut self) -> Option<(usize, u32)> {
        let hex = self.next()?.strip_prefix(b"<")?.strip_suffix(b">")?;
        if !matches!(hex.len(), 2 | 4 | 8) || !hex.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        Some((
            hex.len() / 2,
            u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?,
        ))
    }
}

fn decimal_count(raw: &[u8]) -> Option<usize> {
    if raw.is_empty() || !raw.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(raw).ok()?.parse().ok()
}

fn unicode(width: usize, raw: u32) -> Option<char> {
    let text = match width {
        2 => char::from_u32(raw)?,
        4 => {
            let hi = (raw >> 16).checked_sub(0xd800)?;
            let lo = (raw & 0xffff).checked_sub(0xdc00)?;
            if hi > 0x3ff || lo > 0x3ff {
                return None;
            }
            char::from_u32(0x10000 + (hi << 10) + lo)?
        }
        _ => return None,
    };
    (!text.is_control()
        && !matches!(
            text,
            '\u{fb00}'..='\u{fb06}' | '\u{fffd}' | '\u{feff}' | '\u{2028}' | '\u{2029}'
        ))
    .then_some(text)
}

fn parse_cmap(
    bytes: &[u8],
    width: usize,
    mappings_memory: &mut Reservation<'_>,
    workspace: &Workspace,
) -> RecognitionResult<BTreeMap<u16, char>> {
    use NativeTextRefusal::Mapping;
    if bytes.len() > MAX_CMAP_BYTES || !bytes.is_ascii() {
        return Err(Mapping.into());
    }
    let mut cmap = CMap { rest: bytes };
    cmap.expect(&[b"/CIDInit", b"/ProcSet", b"findresource", b"begin"])
        .ok_or(Mapping)?;
    cmap.number().filter(|count| *count <= 100).ok_or(Mapping)?;
    cmap.expect(&[b"dict", b"begin", b"begincmap"])
        .ok_or(Mapping)?;
    let mut seen = BTreeSet::new();
    let mut space = false;
    let mut mappings = BTreeMap::new();
    let mut reverse = BTreeSet::new();
    let mut reverse_memory = workspace.scope();
    loop {
        let token = cmap.next().ok_or(Mapping)?;
        if token.starts_with(b"/")
            && !reverse_memory
                .insert(&mut seen, token)
                .map_err(|_| Budget)?
        {
            return Err(Mapping.into());
        }
        match token {
            b"endcmap" => break,
            b"/CMapType" => cmap.expect(&[b"2", b"def"]).ok_or(Mapping)?,
            b"/CMapName" => {
                let name = cmap
                    .next()
                    .and_then(|token| token.strip_prefix(b"/"))
                    .ok_or(Mapping)?;
                if name.is_empty()
                    || !name
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(byte))
                {
                    return Err(Mapping.into());
                }
                cmap.expect(&[b"def"]).ok_or(Mapping)?;
            }
            b"/CIDSystemInfo" => {
                cmap.expect(&[b"<<", b"/Registry"]).ok_or(Mapping)?;
                for (index, next) in [b"/Ordering".as_slice(), b"/Supplement"]
                    .into_iter()
                    .enumerate()
                {
                    let string = cmap
                        .next()
                        .and_then(|token| token.strip_prefix(b"(")?.strip_suffix(b")"))
                        .ok_or(Mapping)?;
                    if string.is_empty() || !string.iter().all(u8::is_ascii_alphanumeric) {
                        return Err(Mapping.into());
                    }
                    cmap.expect(&[next]).ok_or(Mapping)?;
                    if index == 1 {
                        cmap.number().ok_or(Mapping)?;
                    }
                }
                cmap.expect(&[b">>", b"def"]).ok_or(Mapping)?;
            }
            _ => {
                let count = decimal_count(token)
                    .filter(|count| (1..=MAX_MAP_ENTRIES).contains(count))
                    .ok_or(Mapping)?;
                match cmap.next().ok_or(Mapping)? {
                    b"begincodespacerange" if !space && count == 1 && mappings.is_empty() => {
                        let full = if width == 1 { 0xff } else { 0xffff };
                        if cmap.hex().ok_or(Mapping)? != (width, 0)
                            || cmap.hex().ok_or(Mapping)? != (width, full)
                        {
                            return Err(Mapping.into());
                        }
                        cmap.expect(&[b"endcodespacerange"]).ok_or(Mapping)?;
                        space = true;
                    }
                    kind @ (b"beginbfchar" | b"beginbfrange") if space => {
                        let range = kind == b"beginbfrange";
                        for _ in 0..count {
                            let (size, low) = cmap.hex().ok_or(Mapping)?;
                            let (high_size, high) = if range {
                                cmap.hex().ok_or(Mapping)?
                            } else {
                                (size, low)
                            };
                            if size != width
                                || high_size != width
                                || high < low
                                || high - low + 1 > (MAX_MAP_ENTRIES - mappings.len()) as u32
                            {
                                return Err(Mapping.into());
                            }
                            let (utf_size, start) = cmap.hex().ok_or(Mapping)?;
                            for code in low..=high {
                                let text = start
                                    .checked_add(code - low)
                                    .and_then(|scalar| unicode(utf_size, scalar))
                                    .ok_or(Mapping)?;
                                if mappings.contains_key(&(code as u16))
                                    || !reverse_memory
                                        .insert(&mut reverse, text)
                                        .map_err(|_| Budget)?
                                {
                                    return Err(Mapping.into());
                                }
                                mappings_memory
                                    .tree_entry::<(u16, char)>()
                                    .map_err(|_| Budget)?;
                                mappings.insert(code as u16, text);
                            }
                        }
                        cmap.expect(&[if range { b"endbfrange" } else { b"endbfchar" }])
                            .ok_or(Mapping)?;
                    }
                    _ => return Err(Mapping.into()),
                }
            }
        }
    }
    cmap.expect(&[
        b"CMapName",
        b"currentdict",
        b"/CMap",
        b"defineresource",
        b"pop",
        b"end",
        b"end",
    ])
    .ok_or(Mapping)?;
    while let Some(end) = cmap
        .rest
        .iter()
        .position(|byte| matches!(byte, b'\n' | b'\r'))
    {
        let line = cmap.rest[..end].trim_ascii();
        if !line.is_empty() && !line.starts_with(b"%") {
            return Err(Mapping.into());
        }
        cmap.rest = &cmap.rest[end + 1..];
    }
    let tail = cmap.rest.trim_ascii();
    (!mappings.is_empty()
        && seen.contains(b"/CMapName".as_slice())
        && seen.contains(b"/CMapType".as_slice())
        && (tail.is_empty() || tail.starts_with(b"%")))
    .then_some(mappings)
    .ok_or_else(|| Mapping.into())
}

fn font_width(value: &PdfValue) -> Option<i64> {
    let width = match value {
        PdfValue::Integer(width) => width.checked_mul(SCALE)?,
        PdfValue::ExactReal(width) => milli(width.as_bytes())?,
        _ => return None,
    };
    (width > 0 && width <= 10_000 * SCALE).then_some(width)
}

fn cid_widths(
    graph: &NativePdfGraph,
    dict: &Dict,
    memory: &mut Reservation<'_>,
) -> RecognitionResult<(i64, BTreeMap<u16, i64>)> {
    use NativeTextRefusal::Font;
    let default = if dict.contains_key("DW") {
        entry(graph, dict, "DW").and_then(font_width).ok_or(Font)?
    } else {
        1_000 * SCALE
    };
    let mut widths = BTreeMap::new();
    let mut items = if dict.contains_key("W") {
        entry(graph, dict, "W")
            .and_then(PdfValue::as_array)
            .ok_or(Font)?
    } else {
        &[]
    };
    while let [start, next, tail @ ..] = items {
        let start = start
            .as_integer()
            .and_then(|start| u16::try_from(start).ok())
            .ok_or(Font)?;
        if let Some(list) = next.as_array() {
            if list.is_empty() || list.len() > MAX_MAP_ENTRIES - widths.len() {
                return Err(Font.into());
            }
            for (index, width) in list.iter().enumerate() {
                let cid = start.checked_add(index as u16).ok_or(Font)?;
                memory.tree_entry::<(u16, i64)>().map_err(|_| Budget)?;
                if widths.insert(cid, font_width(width).ok_or(Font)?).is_some() {
                    return Err(Font.into());
                }
            }
            items = tail;
        } else {
            let end = next
                .as_integer()
                .and_then(|end| u16::try_from(end).ok())
                .ok_or(Font)?;
            let [width, tail @ ..] = tail else {
                return Err(Font.into());
            };
            let width = font_width(width).ok_or(Font)?;
            if end < start || usize::from(end - start) + 1 > MAX_MAP_ENTRIES - widths.len() {
                return Err(Font.into());
            }
            for cid in start..=end {
                memory.tree_entry::<(u16, i64)>().map_err(|_| Budget)?;
                if widths.insert(cid, width).is_some() {
                    return Err(Font.into());
                }
            }
            items = tail;
        }
    }
    items
        .is_empty()
        .then_some((default, widths))
        .ok_or_else(|| Font.into())
}

fn win_ansi(code: u16) -> Option<char> {
    const HIGH: [char; 32] = [
        '€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0',
        '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\0', 'ž', 'Ÿ',
    ];
    match code {
        32..=126 | 161..=172 | 174..=255 => char::from_u32(u32::from(code)),
        128..=159 => Some(HIGH[usize::from(code - 128)]).filter(|text| *text != '\0'),
        _ => None,
    }
}

struct Glyph {
    scalar: char,
    width: i64,
    ink: Option<Rect>,
}

struct Font {
    profile: NativeTextProfile,
    code_width: usize,
    upem: i64,
    codes: BTreeMap<u16, Glyph>,
    reverse: BTreeMap<char, u16>,
}

enum GidMap<'a> {
    Unicode,
    Identity,
    Stream(&'a [u8]),
}

fn font_profile(
    graph: &NativePdfGraph,
    font: &Dict,
    workspace: &Workspace,
    retained: &mut Reservation<'_>,
) -> RecognitionResult<Font> {
    use NativeTextRefusal::{Font as FontError, Mapping};
    let (profile, widths_dict, code_width, gid_map) =
        match (name(graph, font, "Subtype"), name(graph, font, "Encoding")) {
            (Some("TrueType"), Some("WinAnsiEncoding")) => {
                (NativeTextProfile::TrueTypeWinAnsi, font, 1, GidMap::Unicode)
            }
            (Some("Type0"), Some("Identity-H")) => {
                let Some([child]) =
                    entry(graph, font, "DescendantFonts").and_then(PdfValue::as_array)
                else {
                    return Err(FontError.into());
                };
                let child = value(graph, child)
                    .and_then(PdfValue::as_dictionary)
                    .ok_or(FontError)?;
                let info = entry(graph, child, "CIDSystemInfo")
                    .and_then(PdfValue::as_dictionary)
                    .ok_or(FontError)?;
                if name(graph, child, "Subtype") != Some("CIDFontType2")
                    || entry(graph, info, "Registry").and_then(PdfValue::as_bytes) != Some(b"Adobe")
                    || entry(graph, info, "Ordering").and_then(PdfValue::as_bytes)
                        != Some(b"Identity")
                    || !entry(graph, info, "Supplement")
                        .and_then(PdfValue::as_integer)
                        .is_some_and(|supplement| supplement >= 0)
                {
                    return Err(FontError.into());
                }
                let map = if !child.contains_key("CIDToGIDMap")
                    || name(graph, child, "CIDToGIDMap") == Some("Identity")
                {
                    GidMap::Identity
                } else {
                    let stream = entry(graph, child, "CIDToGIDMap")
                        .and_then(stream)
                        .filter(|stream| local_stream(stream))
                        .ok_or(Mapping)?;
                    let data = &stream.data;
                    if data.is_empty() || data.len() % 2 != 0 || data.len() > 2 * 65_536 {
                        return Err(Mapping.into());
                    }
                    GidMap::Stream(data)
                };
                (NativeTextProfile::IdentityHCidFontType2, child, 2, map)
            }
            _ => return Err(FontError.into()),
        };
    let descriptor = entry(graph, widths_dict, "FontDescriptor")
        .and_then(PdfValue::as_dictionary)
        .ok_or(FontError)?;
    let base_font = name(graph, widths_dict, "BaseFont").ok_or(FontError)?;
    if descriptor.contains_key("FontFile")
        || descriptor.contains_key("FontFile3")
        || name(graph, descriptor, "FontName") != Some(base_font)
    {
        return Err(FontError.into());
    }
    if code_width == 1 {
        let flags = entry(graph, descriptor, "Flags")
            .and_then(PdfValue::as_integer)
            .ok_or(FontError)?;
        if flags & 36 != 32 {
            return Err(FontError.into());
        }
    }
    let embedded = entry(graph, descriptor, "FontFile2")
        .and_then(stream)
        .ok_or(FontError)?;
    if !local_stream(embedded)
        || entry(graph, &embedded.dict, "Length1").and_then(PdfValue::as_integer)
            != i64::try_from(embedded.data.len()).ok()
        || !embedded.data.starts_with(&[0, 1, 0, 0])
    {
        return Err(FontError.into());
    }
    let face = Face::parse(&embedded.data, 0).map_err(|_| FontError)?;
    let tables = face.tables();
    let glyf = tables.glyf.ok_or(FontError)?;
    let raw = face.raw_face();
    if [
        b"fvar", b"gvar", b"COLR", b"SVG ", b"sbix", b"CBDT", b"EBDT", b"bdat", b"CFF ", b"CFF2",
    ]
    .iter()
    .any(|tag| raw.table(ttf_parser::Tag::from_bytes(tag)).is_some())
    {
        return Err(FontError.into());
    }
    let cmap = tables.cmap.ok_or(FontError)?;
    if cmap.subtables.len() > 32 {
        return Err(FontError.into());
    }
    let cmap_stream = entry(graph, font, "ToUnicode")
        .and_then(stream)
        .ok_or(Mapping)?;
    if !local_stream(cmap_stream) || cmap_stream.dict.contains_key("UseCMap") {
        return Err(Mapping.into());
    }
    if let GidMap::Stream(bytes) = gid_map {
        let mut seen = BTreeSet::new();
        let mut memory = workspace.scope();
        for pair in bytes.chunks_exact(2) {
            let glyph = u16::from_be_bytes([pair[0], pair[1]]);
            if glyph >= face.number_of_glyphs()
                || (glyph != 0 && !memory.insert(&mut seen, glyph).map_err(|_| Budget)?)
            {
                return Err(Mapping.into());
            }
        }
    }
    let mut unicode_memory = workspace.scope();
    let unicode = parse_cmap(
        &cmap_stream.data,
        code_width,
        &mut unicode_memory,
        workspace,
    )?;
    let mut widths_memory = workspace.scope();
    let (default_width, widths) = if code_width == 1 {
        let first = entry(graph, widths_dict, "FirstChar")
            .and_then(PdfValue::as_integer)
            .ok_or(FontError)?;
        let last = entry(graph, widths_dict, "LastChar")
            .and_then(PdfValue::as_integer)
            .ok_or(FontError)?;
        let values = entry(graph, widths_dict, "Widths")
            .and_then(PdfValue::as_array)
            .ok_or(FontError)?;
        if !(0..=255).contains(&first)
            || !(first..=255).contains(&last)
            || values.len() as i64 != last - first + 1
        {
            return Err(FontError.into());
        }
        let mut widths = BTreeMap::new();
        for (code, value) in (first..=last).zip(values) {
            if let Some(width) = font_width(value) {
                widths_memory
                    .tree_entry::<(u16, i64)>()
                    .map_err(|_| Budget)?;
                widths.insert(code as u16, width);
            }
        }
        (0, widths)
    } else {
        cid_widths(graph, widths_dict, &mut widths_memory)?
    };

    let mut codes = BTreeMap::new();
    let mut reverse = BTreeMap::new();
    let mut gids = BTreeSet::new();
    let mut gids_memory = workspace.scope();
    let loca = ttf_parser::loca::Table::parse(
        tables.maxp.number_of_glyphs,
        tables.head.index_to_location_format,
        raw.table(ttf_parser::Tag::from_bytes(b"loca"))
            .ok_or(FontError)?,
    )
    .ok_or(FontError)?;
    let glyf_data = raw
        .table(ttf_parser::Tag::from_bytes(b"glyf"))
        .ok_or(FontError)?;
    let mut outline_work = 250_000;
    for (code, scalar) in unicode {
        let pdf_gid = match gid_map {
            GidMap::Unicode => {
                if win_ansi(code) != Some(scalar) {
                    return Err(Mapping.into());
                }
                cmap.subtables
                    .into_iter()
                    .find(|table| {
                        table.platform_id == ttf_parser::PlatformId::Windows
                            && table.encoding_id == 1
                    })
                    .and_then(|table| table.glyph_index(u32::from(scalar)))
                    .ok_or(Mapping)?
            }
            GidMap::Identity => GlyphId(code),
            GidMap::Stream(bytes) => {
                let index = usize::from(code) * 2;
                let pair = bytes.get(index..index + 2).ok_or(Mapping)?;
                GlyphId(u16::from_be_bytes([pair[0], pair[1]]))
            }
        };
        if pdf_gid.0 == 0
            || pdf_gid.0 >= face.number_of_glyphs()
            || !gids_memory
                .insert(&mut gids, pdf_gid.0)
                .map_err(|_| Budget)?
            || face.glyph_index(scalar) != Some(pdf_gid)
            || cmap
                .subtables
                .into_iter()
                .filter(|table| table.is_unicode())
                .filter_map(|table| table.glyph_index(u32::from(scalar)))
                .any(|glyph| glyph != pdf_gid)
        {
            return Err(Mapping.into());
        }
        let width = widths.get(&code).copied().unwrap_or(default_width);
        let advance = i64::from(face.glyph_hor_advance(pdf_gid).ok_or(FontError)?);
        let upem = i64::from(face.units_per_em());
        if width == 0 || advance == 0 || (width * upem - advance * SCALE * SCALE).abs() > 500 * upem
        {
            return Err(FontError.into());
        }
        let ink = if scalar == ' ' && empty_glyph(loca, pdf_gid) {
            None
        } else {
            check_outline(loca, glyf_data, pdf_gid, 0, &mut outline_work).ok_or(FontError)?;
            let header = glyf.bbox(pdf_gid).ok_or(FontError)?;
            let actual = face.glyph_bounding_box(pdf_gid).ok_or(FontError)?;
            if actual.x_min < header.x_min
                || actual.y_min < header.y_min
                || actual.x_max > header.x_max
                || actual.y_max > header.y_max
                || header.x_min >= header.x_max
                || header.y_min >= header.y_max
            {
                return Err(FontError.into());
            }
            Some(header)
        };
        retained.tree_entry::<(char, u16)>().map_err(|_| Budget)?;
        retained.tree_entry::<(u16, Glyph)>().map_err(|_| Budget)?;
        reverse.insert(scalar, code);
        codes.insert(code, Glyph { scalar, width, ink });
    }
    Ok(Font {
        profile,
        code_width,
        upem: i64::from(face.units_per_em()),
        codes,
        reverse,
    })
}

fn empty_glyph(loca: ttf_parser::loca::Table<'_>, glyph: GlyphId) -> bool {
    let (index, next) = (glyph.0, glyph.0 + 1);
    match loca {
        ttf_parser::loca::Table::Short(offsets) => offsets
            .get(index)
            .zip(offsets.get(next))
            .is_some_and(|(a, b)| a == b),
        ttf_parser::loca::Table::Long(offsets) => offsets
            .get(index)
            .zip(offsets.get(next))
            .is_some_and(|(a, b)| a == b),
    }
}

fn check_outline(
    loca: ttf_parser::loca::Table<'_>,
    data: &[u8],
    glyph: GlyphId,
    depth: usize,
    work: &mut usize,
) -> Option<()> {
    if depth > 8 {
        return None;
    }
    let glyph_data = data.get(loca.glyph_range(glyph)?)?;
    let word = |offset| {
        let bytes: [u8; 2] = glyph_data.get(offset..offset + 2)?.try_into().ok()?;
        Some(u16::from_be_bytes(bytes))
    };
    let contours = word(0)? as i16;
    *work = work.checked_sub(1)?;
    if contours > 0 {
        let points = usize::from(word(8 + 2 * contours as usize)?) + 1;
        *work = work.checked_sub(points)?;
        return Some(());
    }
    if contours != -1 {
        return None;
    }
    let mut offset = 10;
    loop {
        let flags = word(offset)?;
        if flags & 2 == 0 || flags & !0x1727 != 0 {
            return None;
        }
        check_outline(loca, data, GlyphId(word(offset + 2)?), depth + 1, work)?;
        offset += if flags & 1 == 0 { 6 } else { 8 };
        if flags & 0x20 == 0 {
            return (offset <= glyph_data.len()).then_some(());
        }
    }
}

#[derive(Clone, Copy)]
struct Metrics {
    bounds: [i128; 4],
    advance: i128,
    denominator: i128,
}

impl Metrics {
    fn wire_bounds(self) -> TextResult<Bounds> {
        use NativeTextRefusal::Layout;
        let [left, bottom, right, top] = self.bounds;
        let floor =
            |edge: i128| i64::try_from(edge.div_euclid(self.denominator)).map_err(|_| Layout);
        let ceil =
            |edge: i128| i64::try_from(-(-edge).div_euclid(self.denominator)).map_err(|_| Layout);
        Ok([floor(left)?, floor(bottom)?, ceil(right)?, ceil(top)?])
    }
}

fn contains<T: Ord>(outer: [T; 4], inner: [T; 4]) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

impl Font {
    fn decode<'a>(&self, bytes: &'a [u8]) -> TextResult<impl Iterator<Item = u16> + use<'a>> {
        if bytes.len() % self.code_width != 0 {
            return Err(NativeTextRefusal::Mapping);
        }
        Ok(bytes.chunks_exact(self.code_width).map(|chunk| {
            if let [a, b] = *chunk {
                u16::from_be_bytes([a, b])
            } else {
                u16::from(chunk[0])
            }
        }))
    }

    fn measure(
        &self,
        show: &Show,
        codes: impl Iterator<Item = u16>,
        mut emit: impl FnMut(char),
    ) -> TextResult<Metrics> {
        use NativeTextRefusal::{Layout, Mapping};

        let upem = i128::from(self.upem);
        let width_scale = i128::from(SCALE * SCALE);
        let denominator = width_scale * upem;
        let size = i128::from(show.size);
        let [x, y] = show.origin.map(|point| i128::from(point) * denominator);
        let (mut left, mut right) = (x, x);
        let (mut bottom, mut top) = (i128::MAX, i128::MIN);
        let mut advance = 0;
        let mut text_bytes = 0;
        for code in codes {
            let glyph = self.codes.get(&code).ok_or(Mapping)?;
            text_bytes += glyph.scalar.len_utf8();
            if text_bytes <= NATIVE_MAX_TEXT_BYTES {
                emit(glyph.scalar);
            }
            if let Some(ink) = glyph.ink {
                let from_font = |edge| i128::from(edge) * size * width_scale;
                left = left.min(x + advance + from_font(ink.x_min));
                right = right.max(x + advance + from_font(ink.x_max));
                bottom = bottom.min(y + from_font(ink.y_min));
                top = top.max(y + from_font(ink.y_max));
            }
            advance += i128::from(glyph.width) * size * upem;
        }
        if bottom >= top || text_bytes > NATIVE_MAX_TEXT_BYTES {
            return Err(Layout);
        }
        Ok(Metrics {
            bounds: [left, bottom, right.max(x + advance), top],
            advance,
            denominator,
        })
    }
}

struct ReservedFont<'w> {
    font: Font,
    _memory: Reservation<'w>,
}

struct Recognition<'a, 'w> {
    graph: &'a NativePdfGraph,
    workspace: &'w Workspace,
    fonts: BTreeMap<*const Dict, TextResult<ReservedFont<'w>>>,
    cache_memory: Reservation<'w>,
    work: usize,
}

impl<'a, 'w> Recognition<'a, 'w> {
    fn new(graph: &'a NativePdfGraph, workspace: &'w Workspace) -> Self {
        Self {
            graph,
            workspace,
            fonts: BTreeMap::new(),
            cache_memory: workspace.scope(),
            work: graph.remaining_work(),
        }
    }

    fn font(&mut self, resources: &Dict, name: &str) -> RecognitionResult<&Font> {
        use NativeTextRefusal::Font;
        let fonts = entry(self.graph, resources, "Font")
            .and_then(PdfValue::as_dictionary)
            .ok_or(Font)?;
        let dictionary = entry(self.graph, fonts, name)
            .and_then(PdfValue::as_dictionary)
            .ok_or(Font)?;
        let key = std::ptr::from_ref(dictionary);
        let cached = match self.fonts.entry(key) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                self.work = self.work.checked_sub(250_000).ok_or(Font)?;
                self.cache_memory
                    .tree_entry::<(*const Dict, TextResult<ReservedFont<'w>>)>()
                    .map_err(|_| Budget)?;
                let mut memory = self.workspace.scope();
                let font = match font_profile(self.graph, dictionary, self.workspace, &mut memory) {
                    Ok(font) => Ok(ReservedFont {
                        font,
                        _memory: memory,
                    }),
                    Err(NativeSessionError::Text(error)) => Err(error),
                    Err(error) => return Err(error),
                };
                entry.insert(font)
            }
        };
        cached
            .as_ref()
            .map(|cached| &cached.font)
            .map_err(|error| (*error).into())
    }

    fn page(
        &mut self,
        page: &NativePdfNotePage,
        output_hash: [u8; 32],
        binding: NativeBinding,
        memory: &mut Reservation<'_>,
    ) -> RecognitionResult<Option<RecognizedPage<'a, 'w>>> {
        use NativeTextRefusal::Layout;
        let Some(content) = content(self.graph, page)? else {
            return Ok(None);
        };
        self.work = self
            .work
            .checked_sub(content.stream.data.len())
            .ok_or(Layout)?;
        let shows = shows(&content.stream.data, self.workspace, memory)?;
        if shows.is_empty() {
            return Ok(None);
        }
        let crop = page
            .geometry
            .ok_or(Layout)?
            .bounds()
            .map(|point| point * SCALE);
        let mut text_memory = self.workspace.scope();
        let mut islands = Vec::<Island>::new();
        for show in shows {
            let font = self.font(content.resources, &show.font)?;
            let codes = font.decode(&show.codes)?;
            let max_text = (show.codes.len() / font.code_width * 4).min(NATIVE_MAX_TEXT_BYTES);
            text_memory.grow(max_text).map_err(|_| Budget)?;
            let mut text = String::with_capacity(max_text);
            let metrics = font.measure(&show, codes, |scalar| text.push(scalar))?;
            let bounds = metrics.wire_bounds()?;
            if !contains(crop, bounds)
                || islands
                    .iter()
                    .any(|neighbor| intersects(neighbor.view.bounds, bounds))
            {
                return Err(Layout.into());
            }
            let mut id = blake3::Hasher::new();
            id.update(b"flowpdf:native-text-island:v1");
            id.update(&output_hash);
            id.update(&binding.epoch);
            id.update(&binding.session_id.to_le_bytes());
            id.update(&binding.revision.to_le_bytes());
            id.update(&page.id.0.to_le_bytes());
            id.update(&page.object.object_number.to_le_bytes());
            id.update(&page.object.generation.to_le_bytes());
            id.update(&content.reference.object_number.to_le_bytes());
            id.update(&content.reference.generation.to_le_bytes());
            id.update(&0_u32.to_le_bytes());
            id.update(&(show.operand.start as u64).to_le_bytes());
            id.update(&(show.operand.end as u64).to_le_bytes());
            memory
                .push(
                    &mut islands,
                    Island {
                        view: NativeTextIsland {
                            id: *id.finalize().as_bytes(),
                            text,
                            bounds,
                            profile: font.profile,
                            operator: if show.array {
                                NativeTextOperator::TJ
                            } else {
                                NativeTextOperator::Tj
                            },
                            max_advance: i64::try_from(metrics.advance / metrics.denominator)
                                .map_err(|_| Layout)?,
                        },
                        show,
                        metrics,
                    },
                )
                .map_err(|_| Budget)?;
        }
        Ok(Some(RecognizedPage {
            content,
            islands,
            text_memory,
        }))
    }
}

pub(super) fn project(
    graph: &NativePdfGraph,
    pages: &[NativePdfNotePage],
    output_hash: [u8; 32],
    binding: NativeBinding,
    retained: &mut usize,
) -> RecognitionResult<Vec<NativeTextPage>> {
    let workspace = Workspace::new(*retained);
    let mut recognition = Recognition::new(graph, &workspace);
    let mut projected = workspace.scope();
    let mut result = Vec::new();
    for page in pages {
        let mut memory = workspace.scope();
        let (islands, refusal) = match recognition.page(page, output_hash, binding, &mut memory) {
            Ok(Some(page)) => {
                let count = page.islands.len();
                let view_bytes = count
                    .checked_mul(size_of::<NativeTextIsland>())
                    .ok_or(Budget)?;
                projected.grow(view_bytes).map_err(|_| Budget)?;
                let mut views = Vec::new();
                views.try_reserve_exact(count).map_err(|_| Budget)?;
                projected.absorb(page.text_memory);
                views.extend(page.islands.into_iter().map(|island| island.view));
                (views, None)
            }
            Ok(None) => continue,
            Err(NativeSessionError::Text(error)) => (Vec::new(), Some(error)),
            Err(error) => return Err(error),
        };
        drop(memory);
        projected
            .push(
                &mut result,
                NativeTextPage {
                    page: page.id,
                    islands,
                    refusal,
                },
            )
            .map_err(|_| Budget)?;
    }
    drop(recognition);
    *retained = workspace.remaining();
    Ok(result)
}

pub(super) struct ExpectedText<'a> {
    pub page: NativePageId,
    pub index: usize,
    pub text: &'a str,
    pub new_object: Option<PdfObjectRef>,
    pub original_bounds: Bounds,
}

pub(super) fn stage<'a>(
    graph: &mut NativePdfGraph,
    page: &NativePdfNotePage,
    binding: NativeBinding,
    output_hash: [u8; 32],
    command: &'a NativeTextCommand,
    workspace: &Workspace,
) -> RecognitionResult<ExpectedText<'a>> {
    use super::NativeSessionError::TargetUnavailable;
    use NativeTextRefusal::{Layout, Mapping, Overflow};

    let NativeTextCommand::ReplaceText { island: id, text } = command;
    if text.is_empty() || text.len() > NATIVE_MAX_TEXT_BYTES {
        return Err(Layout.into());
    }
    let mut operand_memory = workspace.scope();
    let (index, span, size, original_reference, copy_on_write, original_bounds, operand) = {
        let mut memory = workspace.scope();
        let mut recognition = Recognition::new(graph, workspace);
        let recognized = recognition
            .page(page, output_hash, binding, &mut memory)?
            .ok_or(TargetUnavailable)?;
        let content = &recognized.content;
        let (index, island) = recognized
            .islands
            .iter()
            .enumerate()
            .find(|(_, island)| island.view.id == *id)
            .ok_or(TargetUnavailable)?;
        let font = recognition.font(content.resources, &island.show.font)?;
        let code_count = text.chars().count();
        memory
            .grow(code_count * size_of::<u16>())
            .map_err(|_| Budget)?;
        let mut codes = Vec::with_capacity(code_count);
        for scalar in text.chars() {
            codes.push(*font.reverse.get(&scalar).ok_or(Mapping)?);
        }
        let replacement = font.measure(&island.show, codes.iter().copied(), |_| {})?;
        if replacement.advance > island.metrics.advance
            || !contains(island.metrics.bounds, replacement.bounds)
        {
            return Err(Overflow.into());
        }
        let operand_len = code_count * font.code_width * 2 + if island.show.array { 4 } else { 2 };
        let span = island.show.operand.clone();
        let size = content.stream.data.len() - span.len() + operand_len;
        if size > MAX_CONTENT_BYTES || size > graph.limits.max_decoded_stream_bytes {
            return Err(Budget);
        }
        operand_memory.grow(operand_len).map_err(|_| Budget)?;
        let mut operand = Vec::with_capacity(operand_len);
        if island.show.array {
            operand.push(b'[');
        }
        operand.push(b'<');
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for code in codes {
            for &byte in &code.to_be_bytes()[2 - font.code_width..] {
                operand.push(HEX[usize::from(byte >> 4)]);
                operand.push(HEX[usize::from(byte & 15)]);
            }
        }
        operand.push(b'>');
        if island.show.array {
            operand.push(b']');
        }
        (
            index,
            span,
            size,
            content.reference,
            content.copy_on_write,
            island.view.bounds,
            operand,
        )
    };
    graph.reserve_replacement_stream(size)?;
    if operand.capacity() > graph.remaining_retained() {
        return Err(Budget);
    }
    let Some(PdfValue::Stream(original)) = graph.objects.get(&original_reference) else {
        return Err(TargetUnavailable);
    };
    let data = &original.data;
    let mut updated = Vec::with_capacity(size);
    updated.extend_from_slice(&data[..span.start]);
    updated.extend_from_slice(&operand);
    updated.extend_from_slice(&data[span.end..]);
    drop(operand);
    drop(operand_memory);
    let stream = PdfValue::Stream(PdfStream {
        dict: BTreeMap::from([("Length".into(), PdfValue::Integer(size as i64))]),
        encoded: updated.clone(),
        data: updated,
        source_offset: 0,
    });
    let new_object = if copy_on_write {
        if graph.objects.len() >= graph.limits.max_objects {
            return Err(Budget);
        }
        let reference = PdfObjectRef {
            object_number: graph
                .objects
                .keys()
                .next_back()
                .and_then(|object| object.object_number.checked_add(1))
                .ok_or(Budget)?,
            generation: 0,
        };
        let Some(PdfValue::Dictionary(dict)) = graph.objects.get_mut(&page.object) else {
            return Err(TargetUnavailable);
        };
        match dict.get_mut("Contents") {
            Some(PdfValue::Array(slots)) => slots[0] = PdfValue::Reference(reference),
            Some(slot) => *slot = PdfValue::Reference(reference),
            None => return Err(TargetUnavailable),
        }
        graph.objects.insert(reference, stream);
        Some(reference)
    } else {
        graph.objects.insert(original_reference, stream);
        None
    };
    Ok(ExpectedText {
        page: page.id,
        index,
        text,
        new_object,
        original_bounds,
    })
}

pub(super) fn verify(
    pages: &[NativeTextPage],
    expected: &ExpectedText<'_>,
) -> Result<[u8; 32], super::NativeSessionError> {
    let island = pages
        .iter()
        .find(|page| page.page == expected.page)
        .and_then(|page| page.islands.get(expected.index));
    if let Some(island) = island.filter(|island| {
        island.text == expected.text && contains(expected.original_bounds, island.bounds)
    }) {
        Ok(island.id)
    } else {
        Err(super::NativeSessionError::Postcondition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_transfers_text_charges_at_exact_fit() {
        let bytes = text_projection_fixture();
        let graph =
            NativePdfGraph::open(&bytes, super::super::NativeEditLimits::default()).unwrap();
        let ample = graph.remaining_retained();
        let mut notes_remaining = ample;
        let pages =
            super::super::notes::project(&graph, &BTreeMap::new(), &mut notes_remaining).unwrap();
        let hash = *blake3::hash(&bytes).as_bytes();
        let binding = NativeBinding {
            epoch: [7; 16],
            session_id: 1,
            origin_hash: hash,
            revision: 0,
        };
        let view_bytes = 4 * size_of::<NativeTextIsland>();
        let fit = {
            let workspace = Workspace::new(ample);
            let mut recognition = Recognition::new(&graph, &workspace);
            let mut scratch = workspace.scope();
            let _recognized = recognition
                .page(&pages[0], hash, binding, &mut scratch)
                .unwrap()
                .unwrap();
            ample - workspace.remaining() + view_bytes
        };
        let mut retained = fit;
        let result = project(&graph, &pages, hash, binding, &mut retained).unwrap();
        let [page] = result.as_slice() else {
            panic!("expected one projected page");
        };
        assert_eq!(page.refusal, None);
        assert_eq!(page.islands.len(), 4);
        assert!(page.islands.iter().all(|island| island.text == "AAAA"));
        let string_bytes: usize = page
            .islands
            .iter()
            .map(|island| island.text.capacity())
            .sum();
        assert_eq!(
            fit - retained,
            result.capacity() * size_of::<NativeTextPage>() + view_bytes + string_bytes
        );

        let mut short = fit - 1;
        assert_eq!(
            project(&graph, &pages, hash, binding, &mut short),
            Err(Budget)
        );
        assert_eq!(short, fit - 1);
    }

    fn text_projection_fixture() -> Vec<u8> {
        use std::io::Write;

        let stream = |entries: &str, data: &[u8]| {
            let mut object =
                format!("<< /Length {} {entries} >>\nstream\n", data.len()).into_bytes();
            object.extend_from_slice(data);
            object.extend_from_slice(b"\nendstream");
            object
        };
        let font = include_bytes!("../../../data/NotoSans-Regular.ttf");
        let content = (1..=4)
            .map(|line| format!("BT /F1 12 Tf 20 {} Td (AAAA) Tj ET\n", line * 20))
            .collect::<String>();
        let objects = [
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
            stream("", content.as_bytes()),
            b"<< /Type /Font /Subtype /TrueType /BaseFont /NotoSans-Regular /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 65 /Widths [639] /FontDescriptor 6 0 R /ToUnicode 7 0 R >>".to_vec(),
            b"<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 8 0 R >>".to_vec(),
            stream("", b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapType 2 def\n/CMapName /Example def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <0041>\nendbfchar\nendcmap CMapName currentdict /CMap defineresource pop\nend end"),
            stream(&format!("/Length1 {}", font.len()), font),
        ];
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let offsets = objects
            .iter()
            .enumerate()
            .map(|(index, object)| {
                let offset = pdf.len();
                writeln!(pdf, "{} 0 obj", index + 1).unwrap();
                pdf.extend_from_slice(object);
                pdf.extend_from_slice(b"\nendobj\n");
                offset
            })
            .collect::<Vec<_>>();
        let xref = pdf.len();
        let size = offsets.len() + 1;
        write!(pdf, "xref\n0 {size}\n0000000000 65535 f \n").unwrap();
        for offset in offsets {
            writeln!(pdf, "{offset:010} 00000 n ").unwrap();
        }
        write!(
            pdf,
            "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .unwrap();
        pdf
    }

    #[test]
    fn cmap_ranges_and_comments_preserve_exact_code_boundaries() {
        let cmap = |map| {
            format!(
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n                 /CMapType 2 def /CMapName /Example def\n                 1 begincodespacerange <00><FF> endcodespacerange\n                 {map}\nendcmap CMapName currentdict /CMap defineresource pop end end"
            )
        };
        let workspace = Workspace::new(4096);
        let parse = |bytes: &[u8]| parse_cmap(bytes, 1, &mut workspace.scope(), &workspace);
        let valid = cmap("1 beginbfrange % contiguous\n <41><43><0041> endbfrange");
        assert_eq!(
            parse(valid.as_bytes()).unwrap(),
            BTreeMap::from([(65, 'A'), (66, 'B'), (67, 'C')])
        );
        let constrained = Workspace::new(1024);
        assert_eq!(
            parse_cmap(valid.as_bytes(), 1, &mut constrained.scope(), &constrained),
            Err(Budget)
        );
        for bad in [
            "1 beginbfchar <41><0041> <42><0042> endbfchar",
            "1 beginbfrange <41><43><D7FF> endbfrange",
            "2 beginbfchar <41><0041> <42><0041> endbfchar",
            "/Untrusted usecmap 1 beginbfchar <41><0041> endbfchar",
        ] {
            assert_eq!(
                parse(cmap(bad).as_bytes()),
                Err(NativeTextRefusal::Mapping.into())
            );
        }
    }

    #[test]
    fn retained_span_excludes_comments_and_neighbor_tokens() {
        let content = b"q\nBT /F1 12 Tf 1 0 0 1 20.125 100 Tm % (decoy)\n[(AA) 0]TJ ET\nQ";
        let workspace = Workspace::new(32 * 1024);
        let shows = shows(content, &workspace, &mut workspace.scope()).unwrap();
        assert_eq!(shows.len(), 1);
        assert_eq!(shows[0].codes, b"AA");
        assert_eq!(&content[shows[0].operand.clone()], b"[(AA) 0]");
        assert_eq!(shows[0].origin, [20_125, 100_000]);
    }

    #[test]
    fn array_workspace_bounds_array_operands_only() {
        assert_eq!(possible_array_items(b"% chart [bars]\r1 2 m", 16), 0);
        assert_eq!(
            possible_array_items(br"(escaped \) [x] (nested)) <5b> 1 2 m", 16),
            0
        );
        assert_eq!(possible_array_items(b"[(a[b]) [] <5b> 7] TJ", 16), 4);
        assert_eq!(possible_array_items(b"[(a[b]) [] <5b> 7] TJ", 2), 2);
    }

    #[test]
    fn text_operands_require_workspace_before_parsing() {
        let content = b"BT /F1 12 Tf [(AA) 0] TJ ET";
        let workspace = Workspace::new(128);
        assert!(matches!(
            shows(content, &workspace, &mut workspace.scope()),
            Err(Budget)
        ));
        assert_eq!(workspace.remaining(), 128);
    }

    #[test]
    fn tj_array_rejects_multiple_strings_even_if_one_is_empty() {
        for content in [
            b"BT /F1 12 Tf [() (AA)] TJ ET".as_slice(),
            b"BT /F1 12 Tf [(AA) ()] TJ ET".as_slice(),
        ] {
            let workspace = Workspace::new(32 * 1024);
            assert!(matches!(
                shows(content, &workspace, &mut workspace.scope()),
                Err(NativeSessionError::Text(NativeTextRefusal::Layout))
            ));
        }
    }
}
