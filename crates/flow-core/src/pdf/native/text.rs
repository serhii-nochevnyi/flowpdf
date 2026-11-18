
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
use super::graph::{NativeEditError, NativePdfGraph, spend};
use super::{NativeBinding, NativePageId};

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
    // keep its original stream while the unique Contents slot is replaced.
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

fn shows(data: &[u8]) -> TextResult<Vec<Show>> {
    use NativeTextRefusal::Layout;
    let limits = PdfReaderLimits {
        max_content_tokens: MAX_TOKENS,
        max_array_items: MAX_TOKENS,
        max_nesting: 2,
        max_string_bytes: NATIVE_MAX_TEXT_BYTES,
        ..Default::default()
    };
    let mut cursor = ContentCursor::new(data, limits);
    let mut result = Vec::new();
    let mut operands = Vec::new();
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
                return Err(Layout);
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
                let [(ContentToken::Name(name), _, _), (_, _, Some(size))] = operands.as_slice()
                else {
                    return Err(Layout);
                };
                if name.is_empty() || name.contains('#') || *size <= 0 {
                    return Err(Layout);
                }
                font = Some((name.clone(), *size));
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
                        return Err(Layout);
                    }
                    4
                } else {
                    0
                };
                let [(_, _, Some(x)), (_, _, Some(y))] = &operands[offset..] else {
                    return Err(Layout);
                };
                origin = [*x, *y];
                positioned = true;
            }
            "Tj" | "TJ" if in_text && !shown => {
                let [(token, span, _)] = operands.as_slice() else {
                    return Err(Layout);
                };
                let array = op == "TJ";
                let codes = match (array, token) {
                    (false, ContentToken::String(bytes)) => bytes.clone(),
                    (true, ContentToken::Array(items)) => {
                        let mut bytes = Vec::new();
                        for item in items {
                            match item {
                                ContentToken::String(chunk) => {
                                    if !bytes.is_empty() || chunk.len() > NATIVE_MAX_TEXT_BYTES {
                                        return Err(Layout);
                                    }
                                    bytes.extend_from_slice(chunk);
                                }
                                ContentToken::Integer(0) => {}
                                _ => return Err(Layout),
                            }
                        }
                        bytes
                    }
                    _ => return Err(Layout),
                };
                let (font, size) = font.take().ok_or(Layout)?;
                if codes.is_empty() || result.len() >= MAX_ISLANDS_PER_PAGE {
                    return Err(Layout);
                }
                result.push(Show {
                    operand: span.clone(),
                    font,
                    size,
                    origin,
                    codes,
                    array,
                });
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
                    return Err(Layout);
                }
            }
            "Do" => return Err(NativeTextRefusal::Ownership),
            _ => return Err(Layout),
        }
        operands.clear();
    }
    if in_text || saves != 0 || !operands.is_empty() {
        return Err(Layout);
    }
    Ok(result)
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

fn parse_cmap(bytes: &[u8], width: usize) -> Option<BTreeMap<u16, char>> {
    if bytes.len() > MAX_CMAP_BYTES || !bytes.is_ascii() {
        return None;
    }
    let mut cmap = CMap { rest: bytes };
    cmap.expect(&[b"/CIDInit", b"/ProcSet", b"findresource", b"begin"])?;
    cmap.number().filter(|count| *count <= 100)?;
    cmap.expect(&[b"dict", b"begin", b"begincmap"])?;
    let mut seen = BTreeSet::new();
    let mut space = false;
    let mut mappings = BTreeMap::new();
    let mut reverse = BTreeSet::new();
    loop {
        let token = cmap.next()?;
        if token.starts_with(b"/") && !seen.insert(token) {
            return None;
        }
        match token {
            b"endcmap" => break,
            b"/CMapType" => cmap.expect(&[b"2", b"def"])?,
            b"/CMapName" => {
                let name = cmap.next()?.strip_prefix(b"/")?;
                if name.is_empty()
                    || !name
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(byte))
                {
                    return None;
                }
                cmap.expect(&[b"def"])?;
            }
            b"/CIDSystemInfo" => {
                cmap.expect(&[b"<<", b"/Registry"])?;
                for (index, next) in [b"/Ordering".as_slice(), b"/Supplement"]
                    .into_iter()
                    .enumerate()
                {
                    let string = cmap.next()?.strip_prefix(b"(")?.strip_suffix(b")")?;
                    if string.is_empty() || !string.iter().all(u8::is_ascii_alphanumeric) {
                        return None;
                    }
                    cmap.expect(&[next])?;
                    if index == 1 {
                        cmap.number()?;
                    }
                }
                cmap.expect(&[b">>", b"def"])?;
            }
            _ => {
                let count =
                    decimal_count(token).filter(|count| (1..=MAX_MAP_ENTRIES).contains(count))?;
                match cmap.next()? {
                    b"begincodespacerange" if !space && count == 1 && mappings.is_empty() => {
                        let full = if width == 1 { 0xff } else { 0xffff };
                        if cmap.hex()? != (width, 0) || cmap.hex()? != (width, full) {
                            return None;
                        }
                        cmap.expect(&[b"endcodespacerange"])?;
                        space = true;
                    }
                    kind @ (b"beginbfchar" | b"beginbfrange") if space => {
                        let range = kind == b"beginbfrange";
                        for _ in 0..count {
                            let (size, low) = cmap.hex()?;
                            let (high_size, high) = if range { cmap.hex()? } else { (size, low) };
                            if size != width
                                || high_size != width
                                || high < low
                                || high - low + 1 > (MAX_MAP_ENTRIES - mappings.len()) as u32
                            {
                                return None;
                            }
                            let (utf_size, start) = cmap.hex()?;
                            for code in low..=high {
                                let text = unicode(utf_size, start.checked_add(code - low)?)?;
                                if mappings.insert(code as u16, text).is_some()
                                    || !reverse.insert(text)
                                {
                                    return None;
                                }
                            }
                        }
                        cmap.expect(&[if range { b"endbfrange" } else { b"endbfchar" }])?;
                    }
                    _ => return None,
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
    ])?;
    while let Some(end) = cmap
        .rest
        .iter()
        .position(|byte| matches!(byte, b'\n' | b'\r'))
    {
        let line = cmap.rest[..end].trim_ascii();
        if !line.is_empty() && !line.starts_with(b"%") {
            return None;
        }
        cmap.rest = &cmap.rest[end + 1..];
    }
    let tail = cmap.rest.trim_ascii();
    (!mappings.is_empty()
        && seen.contains(b"/CMapName".as_slice())
        && seen.contains(b"/CMapType".as_slice())
        && (tail.is_empty() || tail.starts_with(b"%")))
    .then_some(mappings)
}

fn font_width(value: &PdfValue) -> Option<i64> {
    let width = match value {
        PdfValue::Integer(width) => width.checked_mul(SCALE)?,
        PdfValue::ExactReal(width) => milli(width.as_bytes())?,
        _ => return None,
    };
    (width > 0 && width <= 10_000 * SCALE).then_some(width)
}

fn cid_widths(graph: &NativePdfGraph, dict: &Dict) -> Option<(i64, BTreeMap<u16, i64>)> {
    let default = if dict.contains_key("DW") {
        font_width(entry(graph, dict, "DW")?)?
    } else {
        1_000 * SCALE
    };
    let mut widths = BTreeMap::new();
    let mut items = if dict.contains_key("W") {
        entry(graph, dict, "W")?.as_array()?
    } else {
        &[]
    };
    while let [start, next, tail @ ..] = items {
        let start = u16::try_from(start.as_integer()?).ok()?;
        if let Some(list) = next.as_array() {
            if list.is_empty() || list.len() > MAX_MAP_ENTRIES - widths.len() {
                return None;
            }
            for (index, width) in list.iter().enumerate() {
                let cid = start.checked_add(u16::try_from(index).ok()?)?;
                if widths.insert(cid, font_width(width)?).is_some() {
                    return None;
                }
            }
            items = tail;
        } else {
            let end = u16::try_from(next.as_integer()?).ok()?;
            let [width, tail @ ..] = tail else {
                return None;
            };
            let width = font_width(width)?;
            if end < start || usize::from(end - start) + 1 > MAX_MAP_ENTRIES - widths.len() {
                return None;
            }
            for cid in start..=end {
                if widths.insert(cid, width).is_some() {
                    return None;
                }
            }
            items = tail;
        }
    }
    items.is_empty().then_some((default, widths))
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

fn font_profile(graph: &NativePdfGraph, font: &Dict) -> TextResult<Font> {
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
                    return Err(FontError);
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
                    return Err(FontError);
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
                        return Err(Mapping);
                    }
                    GidMap::Stream(data)
                };
                (NativeTextProfile::IdentityHCidFontType2, child, 2, map)
            }
            _ => return Err(FontError),
        };
    let descriptor = entry(graph, widths_dict, "FontDescriptor")
        .and_then(PdfValue::as_dictionary)
        .ok_or(FontError)?;
    let base_font = name(graph, widths_dict, "BaseFont").ok_or(FontError)?;
    if descriptor.contains_key("FontFile")
        || descriptor.contains_key("FontFile3")
        || name(graph, descriptor, "FontName") != Some(base_font)
    {
        return Err(FontError);
    }
    if code_width == 1 {
        let flags = entry(graph, descriptor, "Flags")
            .and_then(PdfValue::as_integer)
            .ok_or(FontError)?;
        if flags & 36 != 32 {
            return Err(FontError);
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
        return Err(FontError);
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
        return Err(FontError);
    }
    let cmap = tables.cmap.ok_or(FontError)?;
    if cmap.subtables.len() > 32 {
        return Err(FontError);
    }
    let cmap_stream = entry(graph, font, "ToUnicode")
        .and_then(stream)
        .ok_or(Mapping)?;
    if !local_stream(cmap_stream) || cmap_stream.dict.contains_key("UseCMap") {
        return Err(Mapping);
    }
    if let GidMap::Stream(bytes) = gid_map {
        let mut seen = BTreeSet::new();
        for pair in bytes.chunks_exact(2) {
            let glyph = u16::from_be_bytes([pair[0], pair[1]]);
            if glyph >= face.number_of_glyphs() || (glyph != 0 && !seen.insert(glyph)) {
                return Err(Mapping);
            }
        }
    }
    let unicode = parse_cmap(&cmap_stream.data, code_width).ok_or(Mapping)?;
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
            return Err(FontError);
        }
        let widths = (first..=last)
            .zip(values)
            .filter_map(|(code, value)| Some((code as u16, font_width(value)?)))
            .collect();
        (0, widths)
    } else {
        cid_widths(graph, widths_dict).ok_or(FontError)?
    };

    let mut codes = BTreeMap::new();
    let mut reverse = BTreeMap::new();
    let mut gids = BTreeSet::new();
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
                    return Err(Mapping);
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
            || !gids.insert(pdf_gid.0)
            || face.glyph_index(scalar) != Some(pdf_gid)
            || cmap
                .subtables
                .into_iter()
                .filter(|table| table.is_unicode())
                .filter_map(|table| table.glyph_index(u32::from(scalar)))
                .any(|glyph| glyph != pdf_gid)
        {
            return Err(Mapping);
        }
        let width = widths.get(&code).copied().unwrap_or(default_width);
        let advance = i64::from(face.glyph_hor_advance(pdf_gid).ok_or(FontError)?);
        let upem = i64::from(face.units_per_em());
        if width == 0 || advance == 0 || (width * upem - advance * SCALE * SCALE).abs() > 500 * upem
        {
            return Err(FontError);
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
                return Err(FontError);
            }
            Some(header)
        };
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
    fn decode(&self, bytes: &[u8]) -> TextResult<Vec<u16>> {
        if bytes.len() % self.code_width != 0 {
            return Err(NativeTextRefusal::Mapping);
        }
        Ok(bytes
            .chunks_exact(self.code_width)
            .map(|chunk| {
                if let [a, b] = *chunk {
                    u16::from_be_bytes([a, b])
                } else {
                    u16::from(chunk[0])
                }
            })
            .collect())
    }

    fn measure(&self, show: &Show, codes: &[u16]) -> TextResult<(String, Metrics)> {
        use NativeTextRefusal::{Layout, Mapping};

        // Keep all lengths exact until projection. The denominator includes
        let upem = i128::from(self.upem);
        let width_scale = i128::from(SCALE * SCALE);
        let denominator = width_scale * upem;
        let size = i128::from(show.size);
        let [x, y] = show.origin.map(|point| i128::from(point) * denominator);
        let (mut left, mut right) = (x, x);
        let (mut bottom, mut top) = (i128::MAX, i128::MIN);
        let mut advance = 0;
        let mut text = String::new();
        for code in codes {
            let glyph = self.codes.get(code).ok_or(Mapping)?;
            text.push(glyph.scalar);
            if let Some(ink) = glyph.ink {
                let from_font = |edge| i128::from(edge) * size * width_scale;
                left = left.min(x + advance + from_font(ink.x_min));
                right = right.max(x + advance + from_font(ink.x_max));
                bottom = bottom.min(y + from_font(ink.y_min));
                top = top.max(y + from_font(ink.y_max));
            }
            advance += i128::from(glyph.width) * size * upem;
        }
        if bottom >= top || text.len() > NATIVE_MAX_TEXT_BYTES {
            return Err(Layout);
        }
        Ok((
            text,
            Metrics {
                bounds: [left, bottom, right.max(x + advance), top],
                advance,
                denominator,
            },
        ))
    }
}

struct Recognition<'a> {
    graph: &'a NativePdfGraph,
    fonts: BTreeMap<*const Dict, TextResult<Font>>,
    work: usize,
}

impl<'a> Recognition<'a> {
    fn new(graph: &'a NativePdfGraph) -> Self {
        Self {
            graph,
            fonts: BTreeMap::new(),
            work: graph.remaining_work(),
        }
    }

    fn font(&mut self, resources: &Dict, name: &str) -> TextResult<&Font> {
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
                entry.insert(font_profile(self.graph, dictionary))
            }
        };
        cached.as_ref().map_err(|error| *error)
    }

    fn page(
        &mut self,
        page: &NativePdfNotePage,
        output_hash: [u8; 32],
        binding: NativeBinding,
    ) -> TextResult<Option<(Content<'a>, Vec<Island>)>> {
        use NativeTextRefusal::Layout;
        let Some(content) = content(self.graph, page)? else {
            return Ok(None);
        };
        self.work = self
            .work
            .checked_sub(content.stream.data.len())
            .ok_or(Layout)?;
        let shows = shows(&content.stream.data)?;
        if shows.is_empty() {
            return Ok(None);
        }
        let crop = page
            .geometry
            .ok_or(Layout)?
            .bounds()
            .map(|point| point * SCALE);
        let mut islands = Vec::<Island>::new();
        for show in shows {
            let font = self.font(content.resources, &show.font)?;
            let codes = font.decode(&show.codes)?;
            let (text, metrics) = font.measure(&show, &codes)?;
            let bounds = metrics.wire_bounds()?;
            if !contains(crop, bounds)
                || islands
                    .iter()
                    .any(|neighbor| intersects(neighbor.view.bounds, bounds))
            {
                return Err(Layout);
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
            islands.push(Island {
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
            });
        }
        Ok(Some((content, islands)))
    }
}

pub(super) fn project(
    graph: &NativePdfGraph,
    pages: &[NativePdfNotePage],
    output_hash: [u8; 32],
    binding: NativeBinding,
    retained: &mut usize,
) -> Result<Vec<NativeTextPage>, NativeEditError> {
    let mut recognition = Recognition::new(graph);
    let mut result = Vec::new();
    for page in pages {
        let (islands, refusal) = match recognition.page(page, output_hash, binding) {
            Ok(Some((_, islands))) => (
                islands
                    .into_iter()
                    .map(|island| island.view)
                    .collect::<Vec<_>>(),
                None,
            ),
            Ok(None) => continue,
            Err(error) => (Vec::new(), Some(error)),
        };
        spend(
            retained,
            128 + islands
                .iter()
                .map(|island| 128 + island.text.len())
                .sum::<usize>(),
        )?;
        result.push(NativeTextPage {
            page: page.id,
            islands,
            refusal,
        });
    }
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
) -> Result<ExpectedText<'a>, super::NativeSessionError> {
    use super::NativeSessionError::{Budget, TargetUnavailable};
    use NativeTextRefusal::{Layout, Mapping, Overflow};

    let NativeTextCommand::ReplaceText { island: id, text } = command;
    if text.is_empty() || text.len() > NATIVE_MAX_TEXT_BYTES {
        return Err(Layout.into());
    }
    let mut recognition = Recognition::new(graph);
    let (content, islands) = recognition
        .page(page, output_hash, binding)?
        .ok_or(TargetUnavailable)?;
    let (index, island) = islands
        .iter()
        .enumerate()
        .find(|(_, island)| island.view.id == *id)
        .ok_or(TargetUnavailable)?;
    let font = recognition.font(content.resources, &island.show.font)?;
    let codes = text
        .chars()
        .map(|scalar| font.reverse.get(&scalar).copied().ok_or(Mapping))
        .collect::<Result<Vec<_>, _>>()?;
    let (_, replacement) = font.measure(&island.show, &codes)?;
    if replacement.advance > island.metrics.advance
        || !contains(island.metrics.bounds, replacement.bounds)
    {
        return Err(Overflow.into());
    }
    let mut operand = Vec::new();
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
    let span = &island.show.operand;
    let data = &content.stream.data;
    let size = data.len() - span.len() + operand.len();
    if size > MAX_CONTENT_BYTES || size > graph.limits.max_decoded_stream_bytes {
        return Err(Budget);
    }
    let mut remaining = graph.remaining_retained();
    spend(&mut remaining, 1024 + size * 2)?;
    let mut updated = Vec::with_capacity(size);
    updated.extend_from_slice(&data[..span.start]);
    updated.extend_from_slice(&operand);
    updated.extend_from_slice(&data[span.end..]);
    let stream = PdfValue::Stream(PdfStream {
        dict: BTreeMap::from([("Length".into(), PdfValue::Integer(size as i64))]),
        encoded: updated.clone(),
        data: updated,
        source_offset: 0,
    });
    let original_reference = content.reference;
    let original_bounds = island.view.bounds;
    let new_object = if content.copy_on_write {
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
    fn cmap_ranges_and_comments_preserve_exact_code_boundaries() {
        let cmap = |map| {
            format!(
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n                 /CMapType 2 def /CMapName /Example def\n                 1 begincodespacerange <00><FF> endcodespacerange\n                 {map}\nendcmap CMapName currentdict /CMap defineresource pop end end"
            )
        };
        let valid = cmap("1 beginbfrange % contiguous\n <41><43><0041> endbfrange");
        assert_eq!(
            parse_cmap(valid.as_bytes(), 1).unwrap(),
            BTreeMap::from([(65, 'A'), (66, 'B'), (67, 'C')])
        );
        for bad in [
            "1 beginbfchar <41><0041> <42><0042> endbfchar",
            "1 beginbfrange <41><43><D7FF> endbfrange",
            "2 beginbfchar <41><0041> <42><0041> endbfchar",
            "/Untrusted usecmap 1 beginbfchar <41><0041> endbfchar",
        ] {
            assert!(parse_cmap(cmap(bad).as_bytes(), 1).is_none());
        }
    }

    #[test]
    fn retained_span_excludes_comments_and_neighbor_tokens() {
        let content = b"q\nBT /F1 12 Tf 1 0 0 1 20.125 100 Tm % (decoy)\n[(AA) 0]TJ ET\nQ";
        let shows = shows(content).unwrap();
        assert_eq!(shows.len(), 1);
        assert_eq!(shows[0].codes, b"AA");
        assert_eq!(&content[shows[0].operand.clone()], b"[(AA) 0]");
        assert_eq!(shows[0].origin, [20_125, 100_000]);
    }
}
