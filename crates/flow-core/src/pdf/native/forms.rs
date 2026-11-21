use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Write as _,
};

use serde::{Deserialize, Deserializer, Serialize};

use super::super::{
    reader::{
        PdfObjectRef as Ref, PdfReadError, PdfStream, PdfValue as Value, WidgetLabelLimits,
        decode_pdf_text_string, map_widget_label,
    },
    scene::{NativePdfAnnotation, NativePdfNotePage, NativeReadOnlyAnnotation},
};
use super::{
    NativeBinding, NativeEditError, NativeSessionError,
    graph::{NativePdfGraph, collect_refs, spend, visit_refs},
    pages::{self, dict, dict_mut, refs},
    text::FieldFont,
};

type Dict = BTreeMap<String, Value>;
pub(super) const MAX_FIELDS: usize = 128;
// @security: Allow a full B-tree node per entry, including child pointers.
pub(super) const FIELD_NAME_ENTRY_BYTES: usize = 512;
pub const NATIVE_MAX_FIELD_NAME_BYTES: usize = 64;
const MAX_NAME: usize = NATIVE_MAX_FIELD_NAME_BYTES;
pub const NATIVE_MAX_FIELD_TEXT_BYTES: usize = 1024;
const MAX_TEXT: usize = NATIVE_MAX_FIELD_TEXT_BYTES;
const FONT: &str = "FlowField";
const DA: &[u8] = b"/FlowField 10 Tf 0 g";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeFieldKind {
    Text,
    Checkbox,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NativeFieldRefusal {
    #[serde(rename = "FLOW_PDF_NATIVE_FIELD_PROFILE")]
    Profile,
    #[serde(rename = "FLOW_PDF_NATIVE_FIELD_REFERENCE")]
    Reference,
    #[serde(rename = "FLOW_PDF_NATIVE_FIELD_APPEARANCE")]
    Appearance,
    #[serde(rename = "FLOW_PDF_NATIVE_FIELD_GLYPH")]
    Glyph,
    #[serde(rename = "FLOW_PDF_NATIVE_FIELD_GEOMETRY")]
    Geometry,
}

impl NativeFieldRefusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Profile => "FLOW_PDF_NATIVE_FIELD_PROFILE",
            Self::Reference => "FLOW_PDF_NATIVE_FIELD_REFERENCE",
            Self::Appearance => "FLOW_PDF_NATIVE_FIELD_APPEARANCE",
            Self::Glyph => "FLOW_PDF_NATIVE_FIELD_GLYPH",
            Self::Geometry => "FLOW_PDF_NATIVE_FIELD_GEOMETRY",
        }
    }
}

use NativeFieldRefusal::{Appearance, Geometry, Glyph, Profile, Reference};
type FieldResult<T> = Result<T, NativeFieldRefusal>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeFieldView {
    pub id: [u8; 32],
    pub object: Ref,
    pub page: u32,
    pub kind: Option<NativeFieldKind>,
    pub name: Option<String>,
    pub rect: Option<[i64; 4]>,
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub can_edit_appearance: bool,
    pub refusal: Option<NativeFieldRefusal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NativeFieldCommand {
    AddTextField {
        page: u32,
        rect: [i64; 4],
        #[serde(deserialize_with = "bounded_name")]
        name: String,
        #[serde(deserialize_with = "bounded_text")]
        value: String,
    },
    AddCheckboxField {
        page: u32,
        rect: [i64; 4],
        #[serde(deserialize_with = "bounded_name")]
        name: String,
        checked: bool,
    },
    SetFieldText {
        field: [u8; 32],
        #[serde(deserialize_with = "bounded_text")]
        value: String,
    },
    SetFieldChecked {
        field: [u8; 32],
        checked: bool,
    },
    MoveField {
        field: [u8; 32],
        page: u32,
        rect: [i64; 4],
    },
    RemoveField {
        field: [u8; 32],
    },
}

fn bounded_string<'de, D: Deserializer<'de>, const LIMIT: usize>(
    deserializer: D,
) -> Result<String, D::Error> {
    struct Bounded<const LIMIT: usize>;
    impl<const LIMIT: usize> serde::de::Visitor<'_> for Bounded<LIMIT> {
        type Value = String;
        fn expecting(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(out, "a string of at most {LIMIT} UTF-8 bytes")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
            if value.len() > LIMIT {
                return Err(E::custom("field input exceeds its byte limit"));
            }
            Ok(value.to_owned())
        }
    }
    deserializer.deserialize_str(Bounded::<LIMIT>)
}

fn bounded_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    bounded_string::<D, MAX_NAME>(d)
}

fn bounded_text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    bounded_string::<D, MAX_TEXT>(d)
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(super) fn checkbox_on_state(states: &Dict) -> Option<&str> {
    if states.len() != 2 || !states.contains_key("Off") {
        return None;
    }
    states
        .keys()
        .map(String::as_str)
        .find(|&state| state != "Off" && valid_name(state))
}

fn valid_text(value: &str) -> bool {
    value.len() <= MAX_TEXT && !value.chars().any(char::is_control)
}

impl NativeFieldCommand {
    pub(super) fn validate(&self) -> FieldResult<()> {
        use NativeFieldCommand::*;
        match self {
            AddTextField { name, .. } | AddCheckboxField { name, .. } if !valid_name(name) => {
                Err(Profile)
            }
            AddTextField { value, .. } | SetFieldText { value, .. } if !valid_text(value) => {
                Err(Glyph)
            }
            _ => Ok(()),
        }
    }

    fn target(&self) -> Option<&[u8; 32]> {
        use NativeFieldCommand::*;
        match self {
            SetFieldText { field, .. }
            | SetFieldChecked { field, .. }
            | MoveField { field, .. }
            | RemoveField { field } => Some(field),
            _ => None,
        }
    }
}

fn name(value: &str) -> Value {
    Value::Name(value.into())
}

fn numbers<const N: usize>(values: [i64; N]) -> Value {
    Value::Array(values.into_iter().map(Value::Integer).collect())
}

fn dictionary<const N: usize>(entries: [(&str, Value); N]) -> Dict {
    let mut values = Dict::new();
    for (key, value) in entries {
        values.insert(key.into(), value);
    }
    values
}

fn decode_saved_text(value: &Value, remaining: &mut usize) -> Result<String, NativeSessionError> {
    let bytes = value.as_bytes().ok_or(Profile)?;
    decode_pdf_text_string(bytes, MAX_TEXT, remaining)
        .map_err(|_| NativeEditError::Budget)?
        .filter(|value| valid_text(value))
        .ok_or_else(|| Profile.into())
}

// @contract: The caller reserves the encoded length before allocating the buffer.
fn encode_saved_text(
    graph: &mut NativePdfGraph,
    text: &str,
    length: usize,
) -> Result<Value, NativeEditError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| NativeEditError::Budget)?;
    graph.charge_page_retained(bytes.capacity() - length)?;
    bytes.extend_from_slice(&[0xfe, 0xff]);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Ok(Value::String(bytes))
}

fn rect(value: &Value) -> Option<[i64; 4]> {
    let [l, b, r, t] = value.as_array()? else {
        return None;
    };
    Some([
        l.as_integer()?,
        b.as_integer()?,
        r.as_integer()?,
        t.as_integer()?,
    ])
}

fn dimensions(page: &NativePdfNotePage, bounds: [i64; 4]) -> FieldResult<[i64; 2]> {
    let [l, b, r, t] = bounds;
    if !bounds
        .into_iter()
        .all(|coord| (-1_000_000..=1_000_000).contains(&coord))
    {
        return Err(Geometry);
    }
    let [pl, pb, pr, pt] = page.geometry.ok_or(Geometry)?.bounds();
    let [width, height] = [r - l, t - b];
    if l < pl || b < pb || r > pr || t > pt || width < 8 || height < 8 {
        return Err(Geometry);
    }
    Ok([width, height])
}

enum Font {
    Simple,
    Embedded(Ref, Box<FieldFont>),
}

fn font_resource(reference: Option<Ref>) -> Value {
    match reference {
        Some(reference) => Value::Reference(reference),
        None => Value::Dictionary(dictionary([
            ("Type", name("Font")),
            ("Subtype", name("Type1")),
            ("BaseFont", name("Helvetica")),
            ("Encoding", name("WinAnsiEncoding")),
        ])),
    }
}

impl Font {
    fn resource(&self) -> Value {
        font_resource(match self {
            Self::Simple => None,
            Self::Embedded(reference, _) => Some(*reference),
        })
    }

    fn operand(
        &self,
        value: &str,
        [width, height]: [i64; 2],
        remaining: &mut usize,
    ) -> Result<String, NativeSessionError> {
        if !valid_text(value) {
            return Err(Glyph.into());
        }
        let hex = match self {
            Self::Embedded(_, font) => {
                let count = value.chars().count();
                if count != 0 {
                    // @invariant: Account for both buffers during growth.
                    let codes = count.next_power_of_two().max(4) * size_of::<u16>();
                    let encoded = (count * 4).next_power_of_two().max(8);
                    let mut scratch = *remaining;
                    spend(&mut scratch, 2 * (codes + encoded))?;
                }
                font.operand(value, width, height)?
            }
            Self::Simple => {
                if !value.bytes().all(|b| (32..=126).contains(&b)) {
                    return Err(Glyph.into());
                }
                if height < 14 || (value.len() as i64) * 11 + 4 > width {
                    return Err(Geometry.into());
                }
                spend(remaining, value.len() * 2)?;
                let mut hex = String::new();
                hex.try_reserve_exact(value.len() * 2)
                    .map_err(|_| NativeSessionError::Budget)?;
                for byte in value.bytes() {
                    write!(hex, "{byte:02X}").map_err(|_| Appearance)?;
                }
                return Ok(hex);
            }
        };
        spend(remaining, hex.capacity())?;
        Ok(hex)
    }
}

struct Form {
    owner: Option<Ref>,
    fields: Vec<Ref>,
    names: BTreeMap<String, Ref>,
    name_inventory_bytes: usize,
    font: Option<FieldResult<Font>>,
    trailer_refs: BTreeSet<Ref>,
}

fn owner_refs(
    graph: &NativePdfGraph,
    owner: Ref,
    work: &mut usize,
    mut remaining: usize,
) -> Result<Vec<Ref>, NativeSessionError> {
    let mut refs = Vec::new();
    collect_refs(
        graph.objects.get(&owner).ok_or(Reference)?,
        &mut refs,
        work,
        &mut remaining,
    )?;
    Ok(refs)
}

fn singular(refs: &[Ref], target: Ref) -> bool {
    refs.iter()
        .filter(|&&reference| reference == target)
        .take(2)
        .count()
        == 1
}

fn metered_refusal(error: NativeEditError, refusal: NativeFieldRefusal) -> NativeSessionError {
    match error {
        NativeEditError::Budget => NativeSessionError::Budget,
        _ => refusal.into(),
    }
}

fn load_form(
    graph: &NativePdfGraph,
    work: &mut usize,
    remaining: &mut usize,
) -> Result<Form, NativeSessionError> {
    let catalog = pages::catalog(graph).map_err(|_| Reference)?;
    let values = dict(graph, catalog).map_err(|_| Reference)?;
    let mut trailer_refs = Vec::new();
    for value in graph.trailer.values() {
        collect_refs(value, &mut trailer_refs, work, remaining)?;
    }
    let trailer_refs: BTreeSet<_> = trailer_refs.into_iter().collect();
    let Some(entry) = values.get("AcroForm") else {
        return Ok(Form {
            owner: None,
            fields: vec![],
            names: BTreeMap::new(),
            name_inventory_bytes: 0,
            font: None,
            trailer_refs,
        });
    };
    let (owner, fields) =
        pages::form_fields(graph, work, remaining).map_err(|error| match error {
            NativeEditError::Budget => NativeSessionError::Budget,
            _ => Reference.into(),
        })?;
    if owner != catalog
        && !graph
            .reverse
            .get(&owner)
            .is_some_and(|owners| owners.len() == 1 && owners.contains(&catalog))
    {
        return Err(Reference.into());
    }
    if owner != catalog
        && (trailer_refs.contains(&owner)
            || !singular(&owner_refs(graph, catalog, work, *remaining)?, owner))
    {
        return Err(Reference.into());
    }
    let values = pages::form_dict(graph, owner).map_err(|_| Reference)?;
    if let Some(container) = values.get("Fields").and_then(Value::as_reference)
        && (trailer_refs.contains(&container)
            || !singular(&owner_refs(graph, owner, work, *remaining)?, container))
    {
        return Err(Reference.into());
    }
    if entry.as_reference() == Some(catalog) || values.contains_key("XFA") {
        return Err(Profile.into());
    }
    for key in ["CO", "SigFlags", "NeedAppearances"] {
        let Some(value) = values.get(key) else {
            continue;
        };
        match (
            key,
            graph
                .resolve_metered(value, work)
                .map_err(|error| metered_refusal(error, Profile))?,
        ) {
            (_, Value::Null)
            | ("SigFlags", Value::Integer(0))
            | ("NeedAppearances", Value::Boolean(false)) => {}
            ("CO", Value::Array(order)) if order.is_empty() => {}
            _ => return Err(Profile.into()),
        }
    }
    for &field in &fields {
        let values = dict(graph, field).map_err(|_| Reference)?;
        if let Some(value) = values.get("FT") {
            match graph.resolve_metered(value, work) {
                Ok(value) if value.as_name() == Some("Sig") => return Err(Profile.into()),
                Err(NativeEditError::Budget) => return Err(NativeSessionError::Budget),
                _ => {}
            }
        }
    }
    let before_names = *remaining;
    let names =
        field_name_inventory(graph, &fields, work, remaining).map_err(|error| match error {
            NativeEditError::Budget => NativeSessionError::Budget,
            _ => Reference.into(),
        })?;
    Ok(Form {
        owner: Some(owner),
        fields,
        names,
        name_inventory_bytes: before_names - *remaining,
        font: None,
        trailer_refs,
    })
}

fn form_font_reference(graph: &NativePdfGraph, form: &Form) -> FieldResult<Option<Ref>> {
    let Some(owner) = form.owner else {
        return Ok(None);
    };
    let values = pages::form_dict(graph, owner).map_err(|_| Reference)?;
    let Some(resources) = values.get("DR") else {
        return Ok(None);
    };
    let resources = resource_value(graph, resources)?;
    let Some(fonts) = resources.get("Font") else {
        return Ok(None);
    };
    let fonts = resource_value(graph, fonts)?;
    let Some(font) = fonts.get(FONT) else {
        return Ok(None);
    };
    if simple_font(font) {
        return Ok(None);
    }
    font.as_reference().map(Some).ok_or(Appearance)
}

fn admit_form_font(
    graph: &NativePdfGraph,
    form: &mut Form,
    reference: Option<Ref>,
    remaining: &mut usize,
) -> Result<(), NativeSessionError> {
    if form.font.is_some() {
        return Ok(());
    }
    let font = match (|| -> Result<Font, NativeSessionError> {
        let Some(reference) = reference else {
            return Ok(Font::Simple);
        };
        let font = dict(graph, reference).map_err(|_| Appearance)?;
        Ok(Font::Embedded(
            reference,
            Box::new(FieldFont::admit(graph, font, remaining)?),
        ))
    })() {
        Ok(font) => Ok(font),
        Err(NativeSessionError::Field(reason)) => Err(reason),
        Err(NativeSessionError::Text(_)) => Err(Appearance),
        Err(error) => return Err(error),
    };
    form.font = Some(font);
    Ok(())
}

impl Form {
    fn release_name_inventory(&mut self, graph: &mut NativePdfGraph, form_bytes: &mut usize) {
        drop(std::mem::take(&mut self.names));
        let bytes = std::mem::take(&mut self.name_inventory_bytes);
        graph.release_page_retained(bytes);
        *form_bytes -= bytes;
    }

    fn release_field_inventory(&mut self, graph: &mut NativePdfGraph, form_bytes: &mut usize) {
        let fields = std::mem::take(&mut self.fields);
        let bytes = fields.capacity() * size_of::<Ref>();
        drop(fields);
        graph.release_page_retained(bytes);
        *form_bytes -= bytes;
    }

    fn text_font(&self) -> FieldResult<&Font> {
        self.font
            .as_ref()
            .ok_or(Appearance)?
            .as_ref()
            .map_err(|&reason| reason)
    }
}

fn simple_font(value: &Value) -> bool {
    value.as_dictionary().is_some_and(|font| {
        font.len() == 4
            && font.get("Type").and_then(Value::as_name) == Some("Font")
            && font.get("Subtype").and_then(Value::as_name) == Some("Type1")
            && font.get("BaseFont").and_then(Value::as_name) == Some("Helvetica")
            && font.get("Encoding").and_then(Value::as_name) == Some("WinAnsiEncoding")
    })
}

#[derive(Clone, PartialEq, Eq)]
enum FieldValue {
    Text(String),
    Checkbox { checked: bool, on: String },
}

impl FieldValue {
    fn retained_value_bytes(&self) -> usize {
        match self {
            Self::Text(text) | Self::Checkbox { on: text, .. } => text.capacity(),
        }
    }

    fn kind(&self) -> NativeFieldKind {
        match self {
            Self::Text(_) => NativeFieldKind::Text,
            Self::Checkbox { .. } => NativeFieldKind::Checkbox,
        }
    }

    fn state(&self) -> Option<&str> {
        match self {
            Self::Text(_) => None,
            Self::Checkbox { checked, on } => Some(if *checked { on } else { "Off" }),
        }
    }
}

fn appearance_dictionary([width, height]: [i64; 2], length: usize, font: Option<Value>) -> Dict {
    let resources = font.map_or_else(Dict::new, |font| {
        dictionary([("Font", Value::Dictionary(dictionary([(FONT, font)])))])
    });
    dictionary([
        ("Type", name("XObject")),
        ("Subtype", name("Form")),
        ("BBox", numbers([0, 0, width, height])),
        ("Resources", Value::Dictionary(resources)),
        ("Length", Value::Integer(length as i64)),
    ])
}

// @invariant: Reserve one B-tree node per appearance dictionary plus keys and names.
const APPEARANCE_DICT_NODE_BYTES: usize = 128 + 12 * size_of::<(String, Value)>();
const APPEARANCE_DICT_BYTES: usize = APPEARANCE_DICT_NODE_BYTES + 64 + 4 * size_of::<Value>();
const APPEARANCE_FONT_BYTES: usize = 2 * APPEARANCE_DICT_NODE_BYTES + 64;
const APPEARANCE_SIMPLE_FONT_BYTES: usize = APPEARANCE_DICT_NODE_BYTES + 64;
const APPEARANCE_STREAM_OBJECT_BYTES: usize = 128 + 12 * size_of::<(Ref, Value)>();
const REPLACEMENT_STREAM_OVERHEAD: usize = 1024;
const NORMAL_APPEARANCE_BYTES: usize = APPEARANCE_DICT_NODE_BYTES + "AP".len() + "N".len();
const TEXT_APPEARANCE_SUFFIX: &[u8] = b"> Tj ET\nQ\n";

fn text_appearance_dictionary_bytes(embedded: bool) -> usize {
    APPEARANCE_DICT_BYTES
        + APPEARANCE_FONT_BYTES
        + if embedded {
            0
        } else {
            APPEARANCE_SIMPLE_FONT_BYTES
        }
}

enum AppearanceKind<'a> {
    Text(&'a str),
    Checkbox(bool),
}

struct AppearanceContent<'a> {
    prefix: [u8; 128],
    prefix_len: usize,
    kind: AppearanceKind<'a>,
}

impl<'a> AppearanceContent<'a> {
    fn new([w, h]: [i64; 2], kind: AppearanceKind<'a>) -> FieldResult<Self> {
        let mut prefix = [0; 128];
        let mut out = &mut prefix[..];
        let [right, top] = [w - 2, h - 2];
        writeln!(out, "q 1 g 0 0 {w} {h} re f 0 G 1 w 1 1 {right} {top} re S")
            .map_err(|_| Appearance)?;
        match kind {
            AppearanceKind::Text(_) => write!(out, "BT /{FONT} 10 Tf 0 g 2 4 Td <"),
            AppearanceKind::Checkbox(false) => writeln!(out, "Q"),
            AppearanceKind::Checkbox(true) => {
                writeln!(out, "2 w 2 2 m {right} {top} l 2 {top} m {right} 2 l S\nQ")
            }
        }
        .map_err(|_| Appearance)?;
        let prefix_len = 128 - out.len();
        Ok(Self {
            prefix,
            prefix_len,
            kind,
        })
    }

    fn prefix(&self) -> &[u8] {
        &self.prefix[..self.prefix_len]
    }

    fn chunks(&self) -> [&[u8]; 3] {
        match self.kind {
            AppearanceKind::Text(hex) => [self.prefix(), hex.as_bytes(), TEXT_APPEARANCE_SUFFIX],
            AppearanceKind::Checkbox(_) => [self.prefix(), &[], &[]],
        }
    }

    fn len(&self) -> usize {
        self.chunks().iter().map(|chunk| chunk.len()).sum()
    }
}

fn same_appearance_dictionary(a: &Dict, b: &Dict) -> bool {
    fn same(left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Name(a), Value::Name(b)) => a == b,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Reference(a), Value::Reference(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
            }
            (Value::Dictionary(a), Value::Dictionary(b)) => same_appearance_dictionary(a, b),
            _ => false,
        }
    }
    a.len() == b.len()
        && b.iter()
            .all(|(key, value)| a.get(key).is_some_and(|entry| same(entry, value)))
}

fn appearance_at<'a>(graph: &'a NativePdfGraph, actual: &Value) -> FieldResult<&'a PdfStream> {
    match actual
        .as_reference()
        .and_then(|reference| graph.objects.get(&reference))
    {
        Some(Value::Stream(stream)) => Ok(stream),
        _ => Err(Appearance),
    }
}

struct Admitted {
    field: Ref,
    name: String,
    value: FieldValue,
    rect: [i64; 4],
}

fn recognize_identity(
    graph: &NativePdfGraph,
    form: &Form,
    page: &NativePdfNotePage,
    widget: Ref,
    work: &mut usize,
    mut remaining: usize,
) -> Result<(Ref, String), NativeSessionError> {
    let widget_data = dict(graph, widget).map_err(|_| Reference)?;
    let field = widget_data
        .get("Parent")
        .map_or(Ok(widget), |parent| parent.as_reference().ok_or(Reference))?;
    let fields = dict(graph, field).map_err(|_| Reference)?;
    if fields.contains_key("Parent") && field != widget
        || widget_data.get("P").and_then(Value::as_reference) != Some(page.object)
    {
        return Err(Reference.into());
    }
    if !matches!(
        fields.get("FT").and_then(Value::as_name),
        Some("Tx" | "Btn")
    ) || fields
        .get("Ff")
        .is_some_and(|flags| !matches!(flags.as_integer(), Some(0 | 2)))
        || field != widget && widget_data.contains_key("T")
    {
        return Err(Profile.into());
    }
    let owner = form.owner.ok_or(Reference)?;
    let fields_slot = pages::form_dict(graph, owner)
        .map_err(|_| Reference)?
        .get("Fields")
        .ok_or(Reference)?;
    // @invariant: The caller charges the array walk, and an indirect field array costs one lookup.
    spend(work, usize::from(fields_slot.as_reference().is_some()))?;
    if pages::annotation_membership(graph, page.object, widget, false)
        .map_err(|error| metered_refusal(error, Reference))?
        != Some(field)
    {
        return Err(Reference.into());
    }
    // @security: Reverse edges identify owners. Count references within each owner to rule out aliases.
    let containers = [
        dict(graph, page.object)
            .map_err(|_| Reference)?
            .get("Annots")
            .and_then(Value::as_reference)
            .map(|container| (page.object, container)),
        fields
            .get("Kids")
            .and_then(Value::as_reference)
            .map(|container| (field, container)),
    ];
    for (owner, container) in containers.into_iter().flatten() {
        if form.trailer_refs.contains(&container)
            || !singular(&owner_refs(graph, owner, work, remaining)?, container)
        {
            return Err(Reference.into());
        }
    }
    for object in BTreeSet::from([widget, field]) {
        if form.trailer_refs.contains(&object) {
            return Err(Reference.into());
        }
        for &owner in graph.reverse.get(&object).ok_or(Reference)? {
            if !singular(&owner_refs(graph, owner, work, remaining)?, object) {
                return Err(Reference.into());
            }
        }
    }
    fields.get("T").and_then(Value::as_bytes).ok_or(Reference)?;
    let field_name = form
        .names
        .iter()
        .find_map(|(name, &owner)| (owner == field && valid_name(name)).then_some(name))
        .ok_or(Reference)?;
    spend(&mut remaining, field_name.len())?;
    Ok((field, field_name.clone()))
}

fn recognize_appearance(
    graph: &NativePdfGraph,
    form: &mut Form,
    page: &NativePdfNotePage,
    widget: Ref,
    (field, field_name): (Ref, String),
    remaining: &mut usize,
) -> Result<Admitted, NativeSessionError> {
    let widget_data = dict(graph, widget).map_err(|_| Reference)?;
    let fields = dict(graph, field).map_err(|_| Reference)?;
    if fields.get("Q").is_some_and(|v| v.as_integer() != Some(0))
        || fields.contains_key("MaxLen")
        || ["MK", "OC", "Border", "BS"]
            .iter()
            .any(|key| widget_data.contains_key(*key))
        || widget_data
            .get("F")
            .is_some_and(|v| !matches!(v.as_integer(), Some(0 | 4)))
        || field != widget
            && ["V", "DV", "FT", "DA", "Ff", "Q", "MaxLen"]
                .iter()
                .any(|key| widget_data.contains_key(*key))
    {
        return Err(Profile.into());
    }
    let bounds = rect(widget_data.get("Rect").ok_or(Geometry)?).ok_or(Geometry)?;
    let size = dimensions(page, bounds)?;
    let ap = widget_data
        .get("AP")
        .and_then(Value::as_dictionary)
        .filter(|ap| ap.len() == 1)
        .and_then(|ap| ap.get("N"))
        .ok_or(Appearance)?;
    let value = match fields.get("FT").and_then(Value::as_name) {
        Some("Tx") => {
            if widget_data.contains_key("AS")
                || fields.get("DA").is_some_and(|v| v.as_bytes() != Some(DA))
            {
                return Err(Appearance.into());
            }
            let stream = appearance_at(graph, ap)?;
            let mut available = *remaining;
            spend(&mut available, field_name.capacity())?;
            let text = decode_saved_text(fields.get("V").ok_or(Profile)?, &mut available)?;
            let reference = form_font_reference(graph, form)?;
            let prefix = AppearanceContent::new(size, AppearanceKind::Text(""))?;
            let dictionary_bytes = text_appearance_dictionary_bytes(reference.is_some());
            spend(&mut available, dictionary_bytes)?;
            let expected =
                appearance_dictionary(size, stream.data.len(), Some(font_resource(reference)));
            let hex = stream
                .data
                .strip_prefix(prefix.prefix())
                .and_then(|data| data.strip_suffix(TEXT_APPEARANCE_SUFFIX));
            let scalars = text.chars().count();
            if !same_appearance_dictionary(&stream.dict, &expected)
                || !hex.is_some_and(|hex| {
                    [scalars * 2, scalars * 4].contains(&hex.len())
                        && hex
                            .iter()
                            .all(|byte| matches!(byte, b'0'..=b'9' | b'A'..=b'F'))
                })
            {
                return Err(Appearance.into());
            }
            drop(expected);
            available += dictionary_bytes;
            let before_default = available;
            let default = fields
                .get("DV")
                .map(|value| decode_saved_text(value, &mut available))
                .transpose()?;
            let default_bytes = before_default - available;
            if form.font.is_none() {
                let before = available;
                admit_form_font(graph, form, reference, &mut available)?;
                spend(remaining, before - available)?;
            }
            if let Some(default) = default {
                let mut scratch = available;
                form.text_font()?.operand(&default, size, &mut scratch)?;
            }
            available += default_bytes;
            let expected_hex = form.text_font()?.operand(&text, size, &mut available)?;
            if hex != Some(expected_hex.as_bytes()) {
                return Err(Appearance.into());
            }
            FieldValue::Text(text)
        }
        Some("Btn") => {
            let states = ap.as_dictionary().ok_or(Appearance)?;
            let on = checkbox_on_state(states).ok_or(Appearance)?;
            let state = fields.get("V").and_then(Value::as_name).ok_or(Profile)?;
            if !states.contains_key(state)
                || widget_data.get("AS").and_then(Value::as_name) != Some(state)
                || fields
                    .get("DV")
                    .is_some_and(|v| !v.as_name().is_some_and(|v| states.contains_key(v)))
            {
                return Err(Appearance.into());
            }
            let mut available = *remaining;
            spend(&mut available, field_name.capacity())?;
            spend(&mut available, APPEARANCE_DICT_BYTES)?;
            for (state, marked) in [("Off", false), (on, true)] {
                let stream = appearance_at(graph, states.get(state).ok_or(Appearance)?)?;
                let content = AppearanceContent::new(size, AppearanceKind::Checkbox(marked))?;
                let expected = appearance_dictionary(size, content.prefix().len(), None);
                if stream.data != content.prefix()
                    || !same_appearance_dictionary(&stream.dict, &expected)
                {
                    return Err(Appearance.into());
                }
            }
            available += APPEARANCE_DICT_BYTES;
            spend(&mut available, on.len())?;
            FieldValue::Checkbox {
                checked: state == on,
                on: on.to_owned(),
            }
        }
        _ => return Err(Profile.into()),
    };
    Ok(Admitted {
        field,
        name: field_name,
        value,
        rect: bounds,
    })
}

fn id(binding: NativeBinding, widget: Ref) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"flowpdf:native-field:v1");
    hash.update(&binding.epoch);
    hash.update(&binding.session_id.to_le_bytes());
    hash.update(&binding.origin_hash);
    hash.update(&binding.revision.to_le_bytes());
    hash.update(&widget.object_number.to_le_bytes());
    hash.update(&widget.generation.to_le_bytes());
    *hash.finalize().as_bytes()
}

pub(super) fn field_name_inventory(
    graph: &NativePdfGraph,
    fields: &[Ref],
    work: &mut usize,
    remaining: &mut usize,
) -> Result<BTreeMap<String, Ref>, NativeEditError> {
    use NativeEditError::{Budget, Unsupported};

    if fields.len() > MAX_FIELDS {
        return Err(Budget);
    }
    spend(work, fields.len())?;
    spend(remaining, fields.len() * FIELD_NAME_ENTRY_BYTES)?;
    let mut names = BTreeMap::new();
    for &field in fields {
        let values = dict(graph, field)?;
        if values.contains_key("Parent")
            || (values.contains_key("Kids") && !values.contains_key("FT"))
        {
            return Err(Unsupported);
        }
        let label = graph.resolve_metered(values.get("T").ok_or(Unsupported)?, work)?;
        let bytes = label.as_bytes().ok_or(Unsupported)?;
        // @security: Meter the full encoded name before decoding it for collision checks.
        spend(work, bytes.len())?;
        let name = decode_pdf_text_string(bytes, usize::MAX, remaining)
            .map_err(|_| Budget)?
            .ok_or(Unsupported)?;
        if names.insert(name, field).is_some() {
            return Err(Unsupported);
        }
    }
    Ok(names)
}

pub(super) fn project(
    graph: &NativePdfGraph,
    pages: &[NativePdfNotePage],
    binding: NativeBinding,
    remaining: &mut usize,
) -> Result<Vec<NativeFieldView>, NativeSessionError> {
    let mut widgets = pages
        .iter()
        .flat_map(|page| {
            page.annotations.iter().filter_map(move |annotation| {
                let NativePdfAnnotation::ReadOnly {
                    object: Some(object),
                    profile,
                    ..
                } = annotation
                else {
                    return None;
                };
                if *profile != NativeReadOnlyAnnotation::Widget
                    && *profile != NativeReadOnlyAnnotation::Appearance
                {
                    return None;
                }
                let data = dict(graph, *object).ok()?;
                (data.get("Subtype").and_then(Value::as_name) == Some("Widget"))
                    .then_some((page, *object, data))
            })
        })
        .peekable();
    if widgets.peek().is_none() {
        return Ok(Vec::new());
    }
    let mut work = graph.remaining_work();
    let mut available = *remaining;
    let (mut form, mut form_bytes) = match load_form(graph, &mut work, &mut available) {
        Ok(form) => (Ok(form), *remaining - available),
        Err(NativeSessionError::Field(reason)) => (Err(reason), 0),
        Err(error) => return Err(error),
    };
    spend(remaining, form_bytes)?;
    let mut result = Vec::new();
    for (page, object, data) in widgets {
        spend(remaining, std::mem::size_of::<NativeFieldView>() * 2)?;
        spend(&mut work, 1)?;
        if let Ok(form) = &form {
            spend(&mut work, form.fields.len() + page.annotations.len())?;
            spend(&mut work, data.get("AP").map_or(0, |_| MAX_TEXT))?;
        }
        let admitted = form
            .as_mut()
            .map_err(|error| NativeSessionError::from(*error))
            .and_then(|form| {
                let identity =
                    recognize_identity(graph, form, page, object, &mut work, *remaining)?;
                let mut available = *remaining;
                let admitted =
                    recognize_appearance(graph, form, page, object, identity, &mut available);
                let bytes = *remaining - available;
                spend(remaining, bytes)?;
                form_bytes += bytes;
                admitted
            });
        let admitted = match admitted {
            Ok(admitted) => Ok(admitted),
            Err(NativeSessionError::Field(reason)) => Err(reason),
            Err(error) => return Err(error),
        };
        let mut view = NativeFieldView {
            id: id(binding, object),
            object,
            page: page.id.0,
            kind: None,
            name: None,
            rect: data
                .get("Rect")
                .and_then(rect)
                .filter(|&bounds| dimensions(page, bounds).is_ok()),
            value: None,
            checked: None,
            can_edit_appearance: admitted.is_ok(),
            refusal: admitted.as_ref().err().copied(),
        };
        if let Ok(admitted) = admitted {
            spend(remaining, admitted.name.capacity())?;
            let text_capacity = match &admitted.value {
                FieldValue::Text(value) => value.capacity(),
                FieldValue::Checkbox { .. } => 0,
            };
            spend(remaining, MAX_TEXT.max(text_capacity))?;
            view.name = Some(admitted.name);
            view.kind = Some(admitted.value.kind());
            match admitted.value {
                FieldValue::Text(value) => view.value = Some(value),
                FieldValue::Checkbox { checked, .. } => view.checked = Some(checked),
            }
        } else {
            let mut available = *remaining;
            view.name = map_widget_label(
                object,
                data.get("T"),
                data.get("Parent").and_then(Value::as_reference),
                WidgetLabelLimits {
                    max_nesting: graph.limits.max_nesting,
                    scratch_bytes: *remaining,
                },
                &mut work,
                |reference| Ok(graph.objects.get(&reference).map(Cow::Borrowed)),
                |bytes| decode_pdf_text_string(bytes, MAX_NAME, &mut available),
            )
            .map_err(|error| match error {
                PdfReadError::TokenLimit
                | PdfReadError::NestingLimit
                | PdfReadError::StreamLimit => NativeSessionError::Budget,
                _ => Reference.into(),
            })?
            .flatten()
            .filter(|name| !name.is_empty() && !name.chars().any(char::is_control));
            if view.name.is_some() {
                *remaining = available;
            }
        }
        result.push(view);
    }
    drop(form);
    *remaining += form_bytes;
    Ok(result)
}

pub(super) struct ExpectedField {
    pub name: String,
    pub page: u32,
    pub rect: [i64; 4],
    value: Option<FieldValue>,
    read_only_name_count: usize,
}

fn read_only_name_count(fields: &[NativeFieldView], name: &str) -> usize {
    fields
        .iter()
        .filter(|field| !field.can_edit_appearance && field.name.as_deref() == Some(name))
        .count()
}

impl ExpectedField {
    pub(super) fn retained_text_bytes(&self) -> usize {
        self.value
            .as_ref()
            .map_or(0, FieldValue::retained_value_bytes)
    }

    pub(super) fn verify(&self, fields: &[NativeFieldView]) -> Result<(), NativeSessionError> {
        let mut matching = fields
            .iter()
            .filter(|field| field.can_edit_appearance && field.name.as_ref() == Some(&self.name));
        let valid = match (&self.value, matching.next()) {
            (None, None) => true,
            (Some(value), Some(field)) => {
                field.page == self.page
                    && field.rect == Some(self.rect)
                    && field.kind == Some(value.kind())
                    && match value {
                        FieldValue::Text(value) => field.value.as_ref() == Some(value),
                        FieldValue::Checkbox { checked, .. } => field.checked == Some(*checked),
                    }
            }
            _ => false,
        };
        if valid
            && matching.next().is_none()
            && read_only_name_count(fields, &self.name) == self.read_only_name_count
        {
            Ok(())
        } else {
            Err(NativeSessionError::Postcondition)
        }
    }
}

fn replace_annots(
    graph: &mut NativePdfGraph,
    form: &Form,
    page: Ref,
    widget: Ref,
    add: bool,
) -> Result<(), NativeSessionError> {
    let work_budget = graph.remaining_work();
    let available = graph.remaining_retained();
    let mut work = work_budget;
    let mut remaining = available;
    if let Some(slot) = dict(graph, page)?.get("Annots") {
        if pages::exclusive_container(graph, page, slot)?
            .as_array()
            .is_none()
        {
            return Err(Reference.into());
        }
        if let Some(container) = slot.as_reference() {
            if form.trailer_refs.contains(&container) {
                return Err(Reference.into());
            }
            let mut occurrences = 0;
            visit_refs(&graph.objects[&page], &mut work, &mut |reference| {
                occurrences += usize::from(reference == container);
                Ok(())
            })?;
            if occurrences != 1 {
                return Err(Reference.into());
            }
        }
    }
    let source = pages::annotation_refs(graph, page, &mut work)?;
    let direct_slots = dict(graph, page)?
        .get("Annots")
        .and_then(Value::as_array)
        .map_or(0, <[_]>::len);
    let keep = |reference| add || reference != widget;
    let count = source.iter().try_fold(usize::from(add), |count, value| {
        let reference = value.as_reference().ok_or(NativeEditError::Unsupported)?;
        count
            .checked_add(usize::from(keep(reference)))
            .ok_or(NativeEditError::Budget)
    })?;
    spend(&mut work, source.len())?;
    graph.reserve_page_value(page, &mut work, &mut remaining)?;
    let installed_bytes = count
        .saturating_sub(direct_slots)
        .checked_mul(size_of::<Value>())
        .ok_or(NativeEditError::Budget)?;
    spend(&mut remaining, installed_bytes)?;
    let mut annots = Vec::new();
    annots
        .try_reserve_exact(count)
        .map_err(|_| NativeEditError::Budget)?;
    annots.extend(
        source
            .iter()
            .filter_map(Value::as_reference)
            .filter(|&reference| keep(reference))
            .map(Value::Reference),
    );
    if add {
        annots.push(Value::Reference(widget));
    }
    graph.charge_page_work(work_budget - work)?;
    graph.charge_page_retained(available - remaining)?;
    let page = dict_mut(graph, page)?;
    if annots.is_empty() {
        page.remove("Annots");
    } else {
        page.insert("Annots".into(), Value::Array(annots));
    }
    Ok(())
}

fn install_form(graph: &mut NativePdfGraph, form: &Form) -> Result<Ref, NativeEditError> {
    if let Some(owner) = form.owner {
        return Ok(owner);
    }
    let catalog = pages::catalog(graph)?;
    graph.reserve_page_copy(catalog)?;
    let owner = pages::allocate(
        graph,
        dictionary([
            ("Fields", refs([])),
            ("NeedAppearances", Value::Boolean(false)),
            ("DA", Value::String(DA.to_vec())),
            (
                "DR",
                Value::Dictionary(dictionary([(
                    "Font",
                    Value::Dictionary(dictionary([(FONT, Font::Simple.resource())])),
                )])),
            ),
        ]),
    )?;
    dict_mut(graph, catalog)?.insert("AcroForm".into(), Value::Reference(owner));
    Ok(owner)
}

fn resource_value<'a>(graph: &'a NativePdfGraph, slot: &'a Value) -> FieldResult<&'a Dict> {
    match slot {
        Value::Dictionary(values) => Ok(values),
        Value::Reference(reference) => dict(graph, *reference).map_err(|_| Appearance),
        _ => Err(Appearance),
    }
}

fn ensure_default_font(
    graph: &mut NativePdfGraph,
    owner: Ref,
    font: &Font,
) -> Result<(), NativeSessionError> {
    let resources = pages::form_dict(graph, owner)?.get("DR");
    let fonts = resources
        .map(|slot| resource_value(graph, slot))
        .transpose()?
        .and_then(|resources| resources.get("Font"));
    if fonts
        .map(|slot| resource_value(graph, slot))
        .transpose()?
        .is_some_and(|fonts| fonts.contains_key(FONT))
    {
        return Ok(());
    }
    let references = [resources, fonts].map(|slot| slot.and_then(Value::as_reference));
    graph.reserve_page_copy(owner)?;
    for reference in references.into_iter().flatten() {
        graph.reserve_page_copy(reference)?;
    }
    // @invariant: Copy shared resource dictionaries before adding a local font name.
    let mut resources = pages::form_dict(graph, owner)?
        .get("DR")
        .map(|slot| resource_value(graph, slot).cloned())
        .transpose()?
        .unwrap_or_default();
    let mut fonts = resources
        .get("Font")
        .map(|slot| resource_value(graph, slot).cloned())
        .transpose()?
        .unwrap_or_default();
    fonts.insert(FONT.into(), font.resource());
    resources.insert("Font".into(), Value::Dictionary(fonts));
    let catalog = pages::catalog(graph)?;
    let values = dict_mut(graph, owner)?;
    let values = if owner == catalog {
        match values.get_mut("AcroForm") {
            Some(Value::Dictionary(form)) => form,
            _ => return Err(Reference.into()),
        }
    } else {
        values
    };
    values.insert("DR".into(), Value::Dictionary(resources));
    Ok(())
}

fn reserve_appearance(
    graph: &mut NativePdfGraph,
    stream_length: usize,
    stream_count: usize,
    other_bytes: usize,
) -> Result<(), NativeEditError> {
    // @invariant: The replacement stream meter already charges for one object.
    let object_bytes = stream_count * APPEARANCE_STREAM_OBJECT_BYTES;
    let other_bytes = other_bytes + object_bytes.saturating_sub(REPLACEMENT_STREAM_OVERHEAD);
    graph.charge_page_retained(other_bytes)?;
    if let Err(error) = graph.reserve_replacement_stream(stream_length) {
        graph.release_page_retained(other_bytes);
        return Err(error);
    }
    Ok(())
}

fn allocate_appearance(
    graph: &mut NativePdfGraph,
    size: [i64; 2],
    content: &AppearanceContent<'_>,
    font: Option<&Font>,
) -> Result<Value, NativeSessionError> {
    let length = content.len();
    let mut data = Vec::new();
    data.try_reserve_exact(length)
        .map_err(|_| NativeEditError::Budget)?;
    for chunk in content.chunks() {
        data.extend_from_slice(chunk);
    }
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(length)
        .map_err(|_| NativeEditError::Budget)?;
    encoded.extend_from_slice(&data);
    let stream = PdfStream {
        dict: appearance_dictionary(size, length, font.map(Font::resource)),
        data,
        encoded,
        source_offset: 0,
    };
    let reference = pages::allocate_value(graph, Value::Stream(stream))?;
    Ok(Value::Reference(reference))
}

fn apply_appearance(
    graph: &mut NativePdfGraph,
    widget: Ref,
    field: Ref,
    form: &Form,
    bounds: [i64; 4],
    size: [i64; 2],
    value: &FieldValue,
    initialize: bool,
) -> Result<(), NativeSessionError> {
    let state = value.state();
    let (normal, saved, default) = match value {
        FieldValue::Text(text) => {
            let font = form.text_font()?;
            let mut remaining = graph.remaining_retained();
            let hex = font.operand(text, size, &mut remaining)?;
            let content = AppearanceContent::new(size, AppearanceKind::Text(&hex))?;
            let dictionary_bytes =
                text_appearance_dictionary_bytes(matches!(font, Font::Embedded(..)));
            let text_length = (1 + text.encode_utf16().count()) * size_of::<u16>();
            let saved_bytes = text_length * (1 + usize::from(initialize));
            let retained_bytes = NORMAL_APPEARANCE_BYTES + saved_bytes;
            // @invariant: The saved strings and normal dictionary can reuse the dropped hex buffer's allowance.
            let scratch_bytes = hex.capacity().saturating_sub(retained_bytes);
            reserve_appearance(
                graph,
                content.len(),
                1,
                dictionary_bytes + retained_bytes + scratch_bytes,
            )?;
            let normal = allocate_appearance(graph, size, &content, Some(font));
            drop(hex);
            graph.release_page_retained(scratch_bytes);
            let normal = normal?;
            let saved = encode_saved_text(graph, text, text_length)?;
            let default = initialize
                .then(|| encode_saved_text(graph, text, text_length))
                .transpose()?;
            (normal, saved, default)
        }
        FieldValue::Checkbox { on, .. } => {
            let off = AppearanceContent::new(size, AppearanceKind::Checkbox(false))?;
            let checked = AppearanceContent::new(size, AppearanceKind::Checkbox(true))?;
            let state_bytes = APPEARANCE_DICT_NODE_BYTES + "Off".len() + on.len();
            reserve_appearance(
                graph,
                off.len() + checked.len(),
                2,
                2 * APPEARANCE_DICT_BYTES + state_bytes + NORMAL_APPEARANCE_BYTES,
            )?;
            let off = allocate_appearance(graph, size, &off, None)?;
            let checked = allocate_appearance(graph, size, &checked, None)?;
            let normal = Value::Dictionary(dictionary([("Off", off), (on.as_str(), checked)]));
            let state = state.unwrap_or("Off");
            (normal, name(state), initialize.then(|| name(state)))
        }
    };
    let field_data = dict_mut(graph, field)?;
    field_data.insert("V".into(), saved);
    if let Some(default) = default {
        field_data.insert("DV".into(), default);
    }
    let data = dict_mut(graph, widget)?;
    data.insert("Rect".into(), numbers(bounds));
    data.insert("AP".into(), Value::Dictionary(dictionary([("N", normal)])));
    if let Some(state) = state {
        data.insert("AS".into(), name(state));
    }
    Ok(())
}

pub(super) fn stage(
    graph: &mut NativePdfGraph,
    pages: &[NativePdfNotePage],
    current: &[NativeFieldView],
    command: &NativeFieldCommand,
) -> Result<ExpectedField, NativeSessionError> {
    command.validate()?;
    let budget = graph.remaining_work();
    let mut work = budget;
    let available = graph.remaining_retained();
    let mut remaining = available;
    let form = load_form(graph, &mut work, &mut remaining)?;
    graph.charge_page_work(budget - work)?;
    let mut form_bytes = available - remaining;
    graph.charge_page_retained(form_bytes)?;
    let result = stage_form(graph, pages, current, command, form, &mut form_bytes);
    graph.release_page_retained(form_bytes);
    result
}

fn owned_field_text(
    graph: &mut NativePdfGraph,
    value: &str,
) -> Result<FieldValue, NativeSessionError> {
    graph.charge_page_retained(value.len())?;
    let mut text = String::new();
    text.try_reserve_exact(value.len())
        .map_err(|_| NativeSessionError::Budget)?;
    graph.charge_page_retained(text.capacity() - value.len())?;
    text.push_str(value);
    Ok(FieldValue::Text(text))
}

fn install_field_inventory(
    graph: &mut NativePdfGraph,
    form: &mut Form,
    form_bytes: &mut usize,
) -> Result<(), NativeSessionError> {
    let owner = form.owner.ok_or(Reference)?;
    let fields = std::mem::take(&mut form.fields);
    let bytes = fields.capacity() * size_of::<Ref>();
    let result = pages::replace_form_fields(graph, owner, fields);
    graph.release_page_retained(bytes);
    *form_bytes -= bytes;
    result.map_err(Into::into)
}

fn stage_form(
    graph: &mut NativePdfGraph,
    pages: &[NativePdfNotePage],
    current: &[NativeFieldView],
    command: &NativeFieldCommand,
    mut form: Form,
    form_bytes: &mut usize,
) -> Result<ExpectedField, NativeSessionError> {
    use NativeFieldCommand::*;
    let page_at = |page: u32| pages.get(page as usize).ok_or(Geometry);
    let target = command
        .target()
        .map(|id| {
            let mut matching = current.iter().filter(|entry| &entry.id == id);
            let view = matching.next().ok_or(Reference)?;
            if matching.next().is_some() {
                return Err(Reference);
            }
            if let Some(reason) = view.refusal {
                return Err(reason);
            }
            Ok(view)
        })
        .transpose()?;
    let (field, widget, page, name, bounds, value) = if let Some(view) = target {
        let old_page = page_at(view.page)?;
        let mut work = graph.remaining_work();
        let budget = work;
        spend(&mut work, old_page.annotations.len() + form.fields.len())?;
        let identity = recognize_identity(
            graph,
            &form,
            old_page,
            view.object,
            &mut work,
            graph.remaining_retained(),
        )?;
        form.release_name_inventory(graph, form_bytes);
        if !matches!(command, RemoveField { .. }) {
            form.release_field_inventory(graph, form_bytes);
        }
        let available = graph.remaining_retained();
        let mut remaining = available;
        let admitted = recognize_appearance(
            graph,
            &mut form,
            old_page,
            view.object,
            identity,
            &mut remaining,
        )?;
        let bytes = available - remaining;
        let value_bytes = admitted.value.retained_value_bytes();
        graph.charge_page_retained(bytes + admitted.name.capacity() + value_bytes)?;
        *form_bytes += bytes;
        graph.charge_page_work(budget - work)?;
        graph.reserve_page_copy(admitted.field)?;
        if admitted.field != view.object {
            graph.reserve_page_copy(view.object)?;
        }
        let (page, bounds, value) = match (command, admitted.value) {
            (RemoveField { .. }, previous) => {
                drop(previous);
                graph.release_page_retained(value_bytes);
                replace_annots(graph, &form, old_page.object, view.object, false)?;
                form.fields.retain(|&entry| entry != admitted.field);
                install_field_inventory(graph, &mut form, form_bytes)?;
                graph.refresh_page_graph(&BTreeSet::from([admitted.field, view.object]))?;
                return Ok(ExpectedField {
                    read_only_name_count: read_only_name_count(current, &admitted.name),
                    name: admitted.name,
                    page: view.page,
                    rect: admitted.rect,
                    value: None,
                });
            }
            (SetFieldText { value, .. }, FieldValue::Text(previous)) => {
                drop(previous);
                graph.release_page_retained(value_bytes);
                (view.page, admitted.rect, owned_field_text(graph, value)?)
            }
            (SetFieldChecked { checked, .. }, FieldValue::Checkbox { on, .. }) => (
                view.page,
                admitted.rect,
                FieldValue::Checkbox {
                    checked: *checked,
                    on,
                },
            ),
            (MoveField { page, rect, .. }, value) => {
                let destination = page_at(*page)?;
                let size = dimensions(destination, *rect)?;
                if value.kind() == NativeFieldKind::Text
                    && let Some(default) = dict(graph, admitted.field)?.get("DV")
                {
                    let mut available = graph.remaining_retained();
                    let default = decode_saved_text(default, &mut available)?;
                    form.text_font()?.operand(&default, size, &mut available)?;
                }
                if *page != view.page {
                    replace_annots(graph, &form, old_page.object, view.object, false)?;
                    replace_annots(graph, &form, destination.object, view.object, true)?;
                    dict_mut(graph, view.object)?
                        .insert("P".into(), Value::Reference(destination.object));
                }
                (*page, *rect, value)
            }
            _ => return Err(Profile.into()),
        };
        (
            admitted.field,
            view.object,
            page,
            admitted.name,
            bounds,
            value,
        )
    } else {
        let (page, rect, field_name) = match command {
            AddTextField {
                page, rect, name, ..
            }
            | AddCheckboxField {
                page, rect, name, ..
            } => (*page, *rect, name),
            _ => return Err(Reference.into()),
        };
        let destination = page_at(page)?;
        dimensions(destination, rect)?;
        if form.fields.len() >= MAX_FIELDS {
            return Err(NativeSessionError::Budget);
        }
        if form.names.contains_key(field_name) {
            return Err(Reference.into());
        }
        form.release_name_inventory(graph, form_bytes);
        let value = match command {
            AddTextField { value, .. } => owned_field_text(graph, value)?,
            AddCheckboxField { checked, .. } => {
                graph.charge_page_retained("Yes".len())?;
                FieldValue::Checkbox {
                    checked: *checked,
                    on: "Yes".into(),
                }
            }
            _ => return Err(Reference.into()),
        };
        if value.kind() == NativeFieldKind::Text {
            let mut remaining = graph.remaining_retained();
            let available = remaining;
            let reference = form_font_reference(graph, &form)?;
            admit_form_font(graph, &mut form, reference, &mut remaining)?;
            let bytes = available - remaining;
            graph.charge_page_retained(bytes)?;
            *form_bytes += bytes;
        }
        graph.charge_page_retained(field_name.len() * 2)?;
        *form_bytes += pages::reserve_form_field_slot(graph, &mut form.fields)?;
        let owner = install_form(graph, &form)?;
        form.owner = Some(owner);
        let ft = match value.kind() {
            NativeFieldKind::Text => "Tx",
            NativeFieldKind::Checkbox => "Btn",
        };
        let widget = dictionary([
            ("Type", name("Annot")),
            ("Subtype", name("Widget")),
            ("P", Value::Reference(destination.object)),
            ("F", Value::Integer(4)),
            ("FT", name(ft)),
            ("T", Value::String(field_name.as_bytes().to_vec())),
        ]);
        let widget = pages::allocate(graph, widget)?;
        form.fields.push(widget);
        install_field_inventory(graph, &mut form, form_bytes)?;
        replace_annots(graph, &form, destination.object, widget, true)?;
        (widget, widget, page, field_name.clone(), rect, value)
    };

    if value.kind() == NativeFieldKind::Text {
        let font = form.text_font()?;
        ensure_default_font(graph, form.owner.ok_or(Reference)?, font)?;
        dict_mut(graph, field)?.insert("DA".into(), Value::String(DA.to_vec()));
    }
    apply_appearance(
        graph,
        widget,
        field,
        &form,
        bounds,
        dimensions(page_at(page)?, bounds)?,
        &value,
        target.is_none(),
    )?;
    graph.refresh_page_graph(&BTreeSet::new())?;
    Ok(ExpectedField {
        read_only_name_count: read_only_name_count(current, &name),
        name,
        page,
        rect: bounds,
        value: Some(value),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{NativeEditLimits, NativePdfSession, tests::redaction_fixture};
    use super::*;

    #[test]
    fn split_widget_label_reserves_unique_child_scratch_before_decode() {
        use super::super::super::reader::WIDGET_LABEL_ENTRY_BYTES;

        let source = redaction_fixture("");
        let limits = NativeEditLimits::default();
        let session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
        let mut page = session.snapshot().scene.pages[0].clone();
        let mut graph = NativePdfGraph::open(&source, limits).unwrap();
        let label = b"recipient";
        let label_ref = pages::allocate_value(&mut graph, Value::String(label.to_vec())).unwrap();
        let parent = pages::allocate(
            &mut graph,
            dictionary([("FT", name("Tx")), ("T", Value::Reference(label_ref))]),
        )
        .unwrap();
        let child = || {
            dictionary([
                ("Subtype", name("Widget")),
                ("Parent", Value::Reference(parent)),
                ("P", Value::Reference(page.object)),
            ])
        };
        let widget = pages::allocate(&mut graph, child()).unwrap();
        let peer = pages::allocate(&mut graph, child()).unwrap();
        let children = pages::allocate_value(&mut graph, refs([widget, peer])).unwrap();
        dict_mut(&mut graph, parent)
            .unwrap()
            .insert("Kids".into(), Value::Reference(children));
        let catalog = pages::catalog(&graph).unwrap();
        dict_mut(&mut graph, catalog).unwrap().insert(
            "AcroForm".into(),
            Value::Dictionary(dictionary([("Fields", refs([parent]))])),
        );
        dict_mut(&mut graph, page.object)
            .unwrap()
            .insert("Annots".into(), refs([widget, peer]));
        graph.refresh_page_graph(&BTreeSet::new()).unwrap();
        page.annotations = [widget, peer]
            .into_iter()
            .map(|object| NativePdfAnnotation::ReadOnly {
                object: Some(object),
                profile: NativeReadOnlyAnnotation::Widget,
                rect: None,
            })
            .collect();

        let peak = 2 * WIDGET_LABEL_ENTRY_BYTES;
        for (scratch, initial_work) in [(peak, 5), (peak - 1, 5), (peak, 4)] {
            let mut available = scratch;
            let mut work = initial_work;
            let result = map_widget_label(
                widget,
                None,
                Some(parent),
                WidgetLabelLimits {
                    max_nesting: 4,
                    scratch_bytes: scratch,
                },
                &mut work,
                |reference| Ok(graph.objects.get(&reference).map(Cow::Borrowed)),
                |bytes| decode_pdf_text_string(bytes, MAX_NAME, &mut available),
            );
            match (scratch, initial_work) {
                (_, 4) => {
                    assert_eq!(result, Err(PdfReadError::TokenLimit));
                    assert_eq!(available, scratch);
                }
                (scratch, _) if scratch < peak => {
                    assert!(matches!(result, Err(PdfReadError::StreamLimit)));
                    assert_eq!(available, scratch);
                }
                _ => {
                    let name = result.unwrap().unwrap().unwrap();
                    assert_eq!(name, "recipient");
                    assert_eq!(available + name.capacity(), scratch);
                    assert_eq!(work, 0);
                }
            }
        }

        let mut inventory_remaining = graph.remaining_retained();
        let initial_work = graph.remaining_work();
        let mut work = initial_work;
        let form = load_form(&graph, &mut work, &mut inventory_remaining).unwrap();
        let inventory_bytes = graph.remaining_retained() - inventory_remaining;
        let work_per_widget = 1 + form.fields.len() + page.annotations.len() + 5;
        let needed_work = initial_work - work + 2 * work_per_widget;
        drop(form);
        graph.charge_page_work(initial_work - needed_work).unwrap();
        let view_bytes = 4 * size_of::<NativeFieldView>();
        let label_bytes = 3 * label.len();
        let allowance = inventory_bytes + view_bytes + label_bytes + peak;
        let binding = session.snapshot().binding;
        let mut remaining = allowance;
        let fields = project(&graph, std::slice::from_ref(&page), binding, &mut remaining).unwrap();
        assert_eq!(fields.len(), 2);
        for (field, object) in fields.iter().zip([widget, peer]) {
            assert_eq!(field.object, object);
            assert_eq!(field.name.as_deref(), Some("recipient"));
            assert!(!field.can_edit_appearance);
        }
        assert_eq!(remaining, allowance - view_bytes - 2 * label_bytes);

        remaining = allowance - 1;
        assert_eq!(
            project(&graph, std::slice::from_ref(&page), binding, &mut remaining),
            Err(NativeSessionError::Budget)
        );
        assert_eq!(remaining, peak - 1);
        graph.charge_page_work(1).unwrap();
        remaining = allowance;
        assert_eq!(
            project(&graph, &[page], binding, &mut remaining),
            Err(NativeSessionError::Budget)
        );
    }

    #[test]
    fn indirect_field_types_spend_the_callers_inventory_and_staging_work() {
        let source = redaction_fixture("");
        let limits = NativeEditLimits::default();
        let session = NativePdfSession::open_scoped(&source, limits, [7; 16]).unwrap();
        let mut graph = NativePdfGraph::open(&source, limits).unwrap();
        let field_type = pages::allocate_value(&mut graph, name("Tx")).unwrap();
        let fields: Vec<_> = ["recipient", "company"]
            .into_iter()
            .map(|label| {
                pages::allocate(
                    &mut graph,
                    dictionary([
                        ("FT", Value::Reference(field_type)),
                        ("T", Value::String(label.as_bytes().to_vec())),
                    ]),
                )
                .unwrap()
            })
            .collect();
        let catalog = pages::catalog(&graph).unwrap();
        dict_mut(&mut graph, catalog).unwrap().insert(
            "AcroForm".into(),
            Value::Dictionary(dictionary([("Fields", refs(fields.iter().copied()))])),
        );
        graph.refresh_page_graph(&BTreeSet::new()).unwrap();

        let initial_work = graph.remaining_work();
        let mut work = initial_work;
        let memory = graph.remaining_retained();
        let mut remaining = memory;
        let indirect = load_form(&graph, &mut work, &mut remaining).unwrap();
        assert_eq!(indirect.fields, fields);
        let needed = initial_work - work;
        for field in &fields {
            dict_mut(&mut graph, *field)
                .unwrap()
                .insert("FT".into(), name("Tx"));
        }
        work = initial_work;
        remaining = memory;
        load_form(&graph, &mut work, &mut remaining).unwrap();
        assert_eq!(needed, initial_work - work + fields.len());
        for field in &fields {
            dict_mut(&mut graph, *field)
                .unwrap()
                .insert("FT".into(), Value::Reference(field_type));
        }

        work = needed;
        remaining = memory;
        load_form(&graph, &mut work, &mut remaining).unwrap();
        assert_eq!(work, 0);
        graph.charge_page_work(initial_work - needed + 1).unwrap();
        let command = NativeFieldCommand::AddCheckboxField {
            page: 0,
            rect: [20, 20, 40, 40],
            name: "confirmed".into(),
            checked: false,
        };
        work = graph.remaining_work();
        remaining = memory;
        assert!(matches!(
            load_form(&graph, &mut work, &mut remaining),
            Err(NativeSessionError::Budget)
        ));
        let object_count = graph.objects.len();
        assert!(matches!(
            stage(&mut graph, &session.snapshot().scene.pages, &[], &command),
            Err(NativeSessionError::Budget)
        ));
        assert_eq!(graph.objects.len(), object_count);
        assert_eq!(graph.remaining_work(), needed - 1);
        assert_eq!(graph.remaining_retained(), memory);
    }

    #[test]
    fn owned_field_text_charges_before_allocation() {
        let source = redaction_fixture("");
        let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
        let label = "Approved for delivery";
        graph
            .charge_page_retained(graph.remaining_retained() - label.len() + 1)
            .unwrap();
        assert!(matches!(
            owned_field_text(&mut graph, label),
            Err(NativeSessionError::Budget)
        ));
        assert_eq!(graph.remaining_retained(), label.len() - 1);
        graph.release_page_retained(1);
        let FieldValue::Text(value) = owned_field_text(&mut graph, label).unwrap() else {
            unreachable!()
        };
        assert_eq!(value, label);
        assert_eq!(graph.remaining_retained(), 0);
    }

    #[test]
    fn field_reference_inventory_reserves_capacity_and_preserves_the_count_limit() {
        let source = redaction_fixture("");
        let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
        let fields: Vec<_> = (0..=MAX_FIELDS)
            .map(|i| {
                pages::allocate(
                    &mut graph,
                    dictionary([
                        ("FT", name("Tx")),
                        ("T", Value::String(format!("item_{i}").into_bytes())),
                    ]),
                )
                .unwrap()
            })
            .collect();
        let catalog = pages::catalog(&graph).unwrap();
        dict_mut(&mut graph, catalog).unwrap().insert(
            "AcroForm".into(),
            Value::Dictionary(dictionary([(
                "Fields",
                refs(fields[..MAX_FIELDS].iter().copied()),
            )])),
        );
        let needed = MAX_FIELDS * size_of::<Ref>();
        for short_by in [1, 0] {
            let mut remaining = needed - short_by;
            let result = pages::form_fields(&graph, &mut graph.remaining_work(), &mut remaining);
            if short_by == 0 {
                let (owner, inventory) = result.unwrap();
                assert_eq!(owner, catalog);
                assert_eq!(inventory, fields[..MAX_FIELDS]);
                assert_eq!(remaining, 0);
            } else {
                assert_eq!(result, Err(NativeEditError::Budget));
                assert_eq!(remaining, needed - short_by);
            }
        }
        pages::replace_form_fields(&mut graph, catalog, fields).unwrap();
        let mut remaining = usize::MAX;
        assert_eq!(
            pages::form_fields(&graph, &mut graph.remaining_work(), &mut remaining),
            Err(NativeEditError::Unsupported)
        );
        assert_eq!(remaining, usize::MAX);
    }

    #[test]
    fn text_appearance_reserves_saved_value_and_default() {
        let source = redaction_fixture("");
        let form = Form {
            owner: None,
            fields: Vec::new(),
            names: BTreeMap::new(),
            name_inventory_bytes: 0,
            font: Some(Ok(Font::Simple)),
            trailer_refs: BTreeSet::new(),
        };
        let value = FieldValue::Text("Paid".into());
        let encoded = b"\xfe\xff\0P\0a\0i\0d";
        let length = b"q 1 g 0 0 80 20 re f 0 G 1 w 1 1 78 18 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <50616964> Tj ET\nQ\n"
            .len();
        let stream_bytes = text_appearance_dictionary_bytes(false)
            + APPEARANCE_STREAM_OBJECT_BYTES.max(REPLACEMENT_STREAM_OVERHEAD)
            + 2 * length;
        for initialize in [true, false] {
            let needed = stream_bytes
                + NORMAL_APPEARANCE_BYTES
                + (1 + usize::from(initialize)) * encoded.len();
            for short_by in [1, 0] {
                let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
                let mut field = dictionary([("FT", name("Tx"))]);
                if !initialize {
                    field.insert("DV".into(), Value::String(b"Unpaid".to_vec()));
                }
                let widget = pages::allocate(&mut graph, field).unwrap();
                let objects = graph.objects.len();
                let remaining = needed - short_by;
                graph
                    .charge_page_retained(graph.remaining_retained() - remaining)
                    .unwrap();
                let result = apply_appearance(
                    &mut graph,
                    widget,
                    widget,
                    &form,
                    [10, 10, 90, 30],
                    [80, 20],
                    &value,
                    initialize,
                );
                let field = dict(&graph, widget).unwrap();
                if short_by == 0 {
                    result.unwrap();
                    let normal = &field["AP"].as_dictionary().unwrap()["N"];
                    let stream = appearance_at(&graph, normal).unwrap();
                    assert_eq!(stream.data.capacity(), length);
                    assert_eq!(stream.encoded.capacity(), length);
                    assert_eq!(field["V"].as_bytes(), Some(encoded.as_slice()));
                    let default = if initialize {
                        encoded.as_slice()
                    } else {
                        b"Unpaid"
                    };
                    assert_eq!(field["DV"].as_bytes(), Some(default));
                    assert_eq!(graph.remaining_retained(), 0);
                } else {
                    assert_eq!(result, Err(NativeSessionError::Budget));
                    assert!(!field.contains_key("V"));
                    assert!(!field.contains_key("AP"));
                    assert_eq!(graph.objects.len(), objects);
                    assert_eq!(graph.remaining_retained(), remaining);
                }
            }
        }
    }

    #[test]
    fn checkbox_reserves_both_appearances_before_allocating_either() {
        let source = redaction_fixture("");
        let form = Form {
            owner: None,
            fields: Vec::new(),
            names: BTreeMap::new(),
            name_inventory_bytes: 0,
            font: None,
            trailer_refs: BTreeSet::new(),
        };
        let state_lengths = [
            b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\nQ\n".len(),
            b"q 1 g 0 0 20 20 re f 0 G 1 w 1 1 18 18 re S\n2 w 2 2 m 18 18 l 2 18 m 18 2 l S\nQ\n"
                .len(),
        ];
        let decoded_bytes = state_lengths.iter().sum::<usize>();
        let needed = 2 * APPEARANCE_DICT_BYTES
            + (2 * APPEARANCE_STREAM_OBJECT_BYTES).max(REPLACEMENT_STREAM_OVERHEAD)
            + 2 * decoded_bytes
            + APPEARANCE_DICT_NODE_BYTES
            + "Off".len()
            + "Yes".len()
            + NORMAL_APPEARANCE_BYTES;
        let value = FieldValue::Checkbox {
            checked: true,
            on: "Yes".into(),
        };
        for short_by in [1, 0] {
            let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
            let widget = pages::allocate(&mut graph, dictionary([("FT", name("Btn"))])).unwrap();
            let objects = graph.objects.len();
            let decoded = graph.remaining_decoded();
            let remaining = needed - short_by;
            graph
                .charge_page_retained(graph.remaining_retained() - remaining)
                .unwrap();
            let result = apply_appearance(
                &mut graph,
                widget,
                widget,
                &form,
                [10, 10, 30, 30],
                [20, 20],
                &value,
                false,
            );
            let widget_data = dict(&graph, widget).unwrap();
            if short_by == 0 {
                result.unwrap();
                let normal = widget_data["AP"].as_dictionary().unwrap()["N"]
                    .as_dictionary()
                    .unwrap();
                for (state, length) in ["Off", "Yes"].into_iter().zip(state_lengths) {
                    let stream = appearance_at(&graph, &normal[state]).unwrap();
                    assert_eq!(stream.data.capacity(), length);
                    assert_eq!(stream.encoded.capacity(), length);
                }
                assert_eq!(graph.remaining_retained(), 0);
                assert_eq!(graph.remaining_decoded(), decoded - decoded_bytes);
            } else {
                assert_eq!(result, Err(NativeSessionError::Budget));
                assert!(!widget_data.contains_key("AP"));
                assert_eq!(graph.objects.len(), objects);
                assert_eq!(graph.remaining_retained(), remaining);
                assert_eq!(graph.remaining_decoded(), decoded);
            }
        }
    }

    #[test]
    fn annots_reserve_the_installed_slots_and_work_before_replacement() {
        let source = redaction_fixture("");
        for (indirect, add) in [(false, false), (false, true), (true, false), (true, true)] {
            for (retained_shortfall, work_shortfall) in [(0, 0), (1, 0), (0, 1)] {
                let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
                let page = graph.pages[0];
                let note =
                    pages::allocate(&mut graph, dictionary([("Subtype", name("Text"))])).unwrap();
                let allocate_widget = |graph: &mut NativePdfGraph| {
                    pages::allocate(graph, dictionary([("Subtype", name("Widget"))])).unwrap()
                };
                let existing_widget = (!add).then(|| allocate_widget(&mut graph));
                let old = refs(std::iter::once(note).chain(existing_widget));
                let old = if indirect {
                    Value::Reference(pages::allocate_value(&mut graph, old).unwrap())
                } else {
                    old
                };
                dict_mut(&mut graph, page)
                    .unwrap()
                    .insert("Annots".into(), old.clone());
                graph.refresh_page_graph(&BTreeSet::new()).unwrap();
                let widget = existing_widget.unwrap_or_else(|| allocate_widget(&mut graph));
                let available = graph.remaining_retained();
                let mut remaining = available;
                let available_work = graph.remaining_work();
                let mut work = available_work;
                let form = load_form(&graph, &mut work, &mut remaining).unwrap();
                remaining = available;
                work = available_work;
                if indirect {
                    visit_refs(&graph.objects[&page], &mut work, &mut |_| Ok(())).unwrap();
                }
                let slots = pages::annotation_refs(&graph, page, &mut work).unwrap();
                spend(&mut work, slots.len()).unwrap();
                graph
                    .reserve_page_value(page, &mut work, &mut remaining)
                    .unwrap();
                let added_slots = match (indirect, add) {
                    (true, _) => 1 + usize::from(add),
                    (false, true) => 1,
                    (false, false) => 0,
                };
                let needed = available - remaining + added_slots * size_of::<Value>();
                let needed_work = available_work - work;
                graph
                    .charge_page_retained(available - needed + retained_shortfall)
                    .unwrap();
                graph.charge_page_work(work + work_shortfall).unwrap();

                let result = replace_annots(&mut graph, &form, page, widget, add);
                let annots = &dict(&graph, page).unwrap()["Annots"];
                if retained_shortfall == 0 && work_shortfall == 0 {
                    result.unwrap();
                    let installed = annots.as_array().unwrap();
                    assert_eq!(installed.len(), 1 + usize::from(add));
                    assert_eq!(installed[0].as_reference(), Some(note));
                    assert_eq!(
                        installed.get(1).and_then(Value::as_reference),
                        add.then_some(widget)
                    );
                    assert_eq!(graph.remaining_retained(), 0);
                    assert_eq!(graph.remaining_work(), 0);
                } else {
                    assert_eq!(result, Err(NativeSessionError::Budget));
                    if indirect {
                        assert_eq!(annots.as_reference(), old.as_reference());
                    } else {
                        let retained = annots.as_array().unwrap();
                        assert_eq!(retained.len(), 2 - usize::from(add));
                        assert_eq!(retained[0].as_reference(), Some(note));
                        assert_eq!(
                            retained.get(1).and_then(Value::as_reference),
                            existing_widget
                        );
                    }
                    assert_eq!(graph.remaining_retained(), needed - retained_shortfall);
                    assert_eq!(graph.remaining_work(), needed_work - work_shortfall);
                }
                if let Some(old_array) = old.as_reference() {
                    let retained = graph.objects[&old_array].as_array().unwrap();
                    assert_eq!(retained.len(), 2 - usize::from(add));
                    assert_eq!(retained[0].as_reference(), Some(note));
                }
            }
        }
    }

    #[test]
    fn indirect_fields_reserve_the_new_array_while_the_old_array_is_live() {
        let source = redaction_fixture("");
        for short_by in [1, 0] {
            let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
            let old_field = pages::allocate(&mut graph, dictionary([("FT", name("Btn"))])).unwrap();
            let new_field = pages::allocate(&mut graph, dictionary([("FT", name("Btn"))])).unwrap();
            let old_array = pages::allocate_value(&mut graph, refs([old_field])).unwrap();
            let owner = pages::catalog(&graph).unwrap();
            dict_mut(&mut graph, owner).unwrap().insert(
                "AcroForm".into(),
                Value::Dictionary(dictionary([("Fields", Value::Reference(old_array))])),
            );
            let mut available = graph.remaining_retained();
            let mut work = graph.remaining_work();
            graph
                .reserve_page_value(owner, &mut work, &mut available)
                .unwrap();
            let copy_bytes = graph.remaining_retained() - available;
            let needed = copy_bytes + 2 * std::mem::size_of::<Value>();
            graph
                .charge_page_retained(graph.remaining_retained() - needed + short_by)
                .unwrap();
            let result = pages::replace_form_fields(&mut graph, owner, vec![old_field, new_field]);
            if short_by == 0 {
                result.unwrap();
                let fields = pages::form_dict(&graph, owner).unwrap()["Fields"]
                    .as_array()
                    .unwrap();
                assert_eq!(fields.len(), 2);
                assert_eq!(graph.objects[&old_array].as_array().unwrap().len(), 1);
                assert_eq!(graph.remaining_retained(), 0);
            } else {
                assert_eq!(result, Err(NativeEditError::Budget));
                assert_eq!(graph.remaining_retained(), needed - 1);
                assert_eq!(
                    pages::form_dict(&graph, owner).unwrap()["Fields"].as_reference(),
                    Some(old_array)
                );
            }
        }
    }

    #[test]
    fn staging_reuses_inventory_allowance_for_new_field_appearances() {
        let bytes = redaction_fixture("");
        let limits = NativeEditLimits::default();
        let session = NativePdfSession::open_scoped(&bytes, limits, [7; 16]).unwrap();
        let mut graph = NativePdfGraph::open(&bytes, limits).unwrap();
        let field = pages::allocate(
            &mut graph,
            dictionary([
                ("FT", name("Tx")),
                ("Ff", Value::Integer(1)),
                ("T", Value::String(vec![b'a'; 8 * 1024])),
            ]),
        )
        .unwrap();
        let catalog = pages::catalog(&graph).unwrap();
        dict_mut(&mut graph, catalog).unwrap().insert(
            "AcroForm".into(),
            Value::Dictionary(dictionary([("Fields", refs([field]))])),
        );
        graph.refresh_page_graph(&BTreeSet::new()).unwrap();
        graph
            .charge_page_retained(graph.remaining_retained() - 25 * 1024)
            .unwrap();

        let expected = stage(
            &mut graph,
            &session.snapshot().scene.pages,
            &[],
            &NativeFieldCommand::AddCheckboxField {
                page: 0,
                rect: [10, 10, 30, 30],
                name: "approval".into(),
                checked: true,
            },
        )
        .unwrap();
        assert_eq!(expected.name, "approval");
        assert!(matches!(
            expected.value,
            Some(FieldValue::Checkbox { checked: true, .. })
        ));
    }
}
