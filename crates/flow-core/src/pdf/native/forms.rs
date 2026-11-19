use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use serde::{Deserialize, Deserializer, Serialize};

use super::super::{
    reader::{PdfObjectRef as Ref, PdfStream, PdfValue as Value},
    scene::{NativePdfAnnotation, NativePdfNotePage, NativeReadOnlyAnnotation},
};
use super::{
    NativeBinding, NativeEditError, NativeSessionError,
    graph::{NativePdfGraph, collect_refs, decode_pdf_doc_encoding, spend},
    pages::{self, dict, dict_mut, refs},
    text::FieldFont,
};

type Dict = BTreeMap<String, Value>;
pub(super) const MAX_FIELDS: usize = 128;
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

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
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
    entries
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}

fn decode(bytes: &[u8], limit: usize) -> Option<String> {
    if bytes.len() > 2 * limit + 2 {
        return None;
    }
    let value = if let Some(utf16) = bytes.strip_prefix(&[0xfe, 0xff]) {
        if utf16.len() % 2 != 0 {
            return None;
        }
        String::from_utf16(
            &utf16
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .ok()?
    } else if let Some(utf8) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        std::str::from_utf8(utf8).ok()?.to_owned()
    } else {
        if !bytes.is_ascii() {
            return None;
        }
        std::str::from_utf8(bytes).ok()?.to_owned()
    };
    (value.len() <= limit).then_some(value)
}

fn editable_name(field: &Dict) -> Option<String> {
    field
        .get("T")
        .and_then(Value::as_bytes)
        .and_then(|bytes| decode(bytes, MAX_NAME))
        .filter(|name| valid_name(name))
}

fn text_string(value: &str) -> Value {
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(value.encode_utf16().flat_map(u16::to_be_bytes));
    Value::String(bytes)
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

impl Font {
    fn resource(&self) -> Value {
        match self {
            Self::Simple => Value::Dictionary(dictionary([
                ("Type", name("Font")),
                ("Subtype", name("Type1")),
                ("BaseFont", name("Helvetica")),
                ("Encoding", name("WinAnsiEncoding")),
            ])),
            Self::Embedded(reference, _) => Value::Reference(*reference),
        }
    }

    fn operand(&self, value: &str, [width, height]: [i64; 2]) -> FieldResult<String> {
        if !valid_text(value) {
            return Err(Glyph);
        }
        match self {
            Self::Embedded(_, font) => font.operand(value, width, height),
            Self::Simple => {
                if !value.bytes().all(|b| (32..=126).contains(&b)) {
                    return Err(Glyph);
                }
                if height < 14 || (value.len() as i64) * 11 + 4 > width {
                    return Err(Geometry);
                }
                let mut hex = String::new();
                for byte in value.bytes() {
                    write!(hex, "{byte:02X}").map_err(|_| Appearance)?;
                }
                Ok(hex)
            }
        }
    }
}

struct Form {
    owner: Option<Ref>,
    fields: Vec<Ref>,
    names: BTreeMap<String, Ref>,
    font: FieldResult<Font>,
    trailer_refs: BTreeSet<Ref>,
}

fn owner_refs(graph: &NativePdfGraph, owner: Ref, work: &mut usize) -> FieldResult<Vec<Ref>> {
    let mut refs = Vec::new();
    collect_refs(
        graph.objects.get(&owner).ok_or(Reference)?,
        &mut refs,
        work,
        &mut graph.remaining_retained(),
    )
    .map_err(|_| Reference)?;
    Ok(refs)
}

fn singular(refs: &[Ref], target: Ref) -> bool {
    refs.iter()
        .filter(|&&reference| reference == target)
        .take(2)
        .count()
        == 1
}

fn load_form(graph: &NativePdfGraph, work: &mut usize) -> FieldResult<Form> {
    let catalog = pages::catalog(graph).map_err(|_| Reference)?;
    let values = dict(graph, catalog).map_err(|_| Reference)?;
    let mut trailer_refs = Vec::new();
    let mut remaining = graph.remaining_retained();
    for value in graph.trailer.values() {
        collect_refs(value, &mut trailer_refs, work, &mut remaining).map_err(|_| Reference)?;
    }
    let trailer_refs: BTreeSet<_> = trailer_refs.into_iter().collect();
    let Some(entry) = values.get("AcroForm") else {
        return Ok(Form {
            owner: None,
            fields: vec![],
            names: BTreeMap::new(),
            font: Ok(Font::Simple),
            trailer_refs,
        });
    };
    let (owner, fields) = pages::form_fields(graph).map_err(|_| Reference)?;
    if fields.len() > MAX_FIELDS
        || owner != catalog
            && !graph
                .reverse
                .get(&owner)
                .is_some_and(|owners| owners.len() == 1 && owners.contains(&catalog))
    {
        return Err(Reference);
    }
    if owner != catalog
        && (trailer_refs.contains(&owner) || !singular(&owner_refs(graph, catalog, work)?, owner))
    {
        return Err(Reference);
    }
    let values = pages::form_dict(graph, owner).map_err(|_| Reference)?;
    if let Some(container) = values.get("Fields").and_then(Value::as_reference)
        && (trailer_refs.contains(&container)
            || !singular(&owner_refs(graph, owner, work)?, container))
    {
        return Err(Reference);
    }
    if entry.as_reference() == Some(catalog) || values.contains_key("XFA") {
        return Err(Profile);
    }
    for key in ["CO", "SigFlags", "NeedAppearances"] {
        let Some(value) = values.get(key) else {
            continue;
        };
        match (
            key,
            graph.resolve_metered(value, work).map_err(|_| Profile)?,
        ) {
            (_, Value::Null)
            | ("SigFlags", Value::Integer(0))
            | ("NeedAppearances", Value::Boolean(false)) => {}
            ("CO", Value::Array(order)) if order.is_empty() => {}
            _ => return Err(Profile),
        }
    }
    let font = (|| {
        let Some(resources) = values.get("DR") else {
            return Ok(Font::Simple);
        };
        let resources = resource_value(graph, resources)?;
        let Some(fonts) = resources.get("Font") else {
            return Ok(Font::Simple);
        };
        let fonts = resource_value(graph, fonts)?;
        let Some(font) = fonts.get(FONT) else {
            return Ok(Font::Simple);
        };
        if simple_font(font) {
            return Ok(Font::Simple);
        }
        let reference = font.as_reference().ok_or(Appearance)?;
        let font = dict(graph, reference).map_err(|_| Appearance)?;
        Ok(Font::Embedded(
            reference,
            Box::new(FieldFont::admit(graph, font).ok_or(Appearance)?),
        ))
    })();
    let mut names = BTreeMap::new();
    for &field in &fields {
        let values = dict(graph, field).map_err(|_| Reference)?;
        if values.get("FT").is_some_and(|value| {
            graph
                .resolve_metered(value, &mut graph.remaining_work())
                .is_ok_and(|value| value.as_name() == Some("Sig"))
        }) {
            return Err(Profile);
        }
        if values.contains_key("Parent")
            || (values.contains_key("Kids") && !values.contains_key("FT"))
        {
            return Err(Reference);
        }
        // Read-only fields still reserve their names in the form namespace.
        let label = graph
            .resolve_metered(values.get("T").ok_or(Reference)?, work)
            .map_err(|_| Reference)?;
        let field_name = decode_field_name(label).ok_or(Reference)?;
        if names.insert(field_name, field).is_some() {
            return Err(Reference);
        }
    }
    Ok(Form {
        owner: Some(owner),
        fields,
        names,
        font,
        trailer_refs,
    })
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

    fn pdf_value(&self) -> Value {
        match self {
            Self::Text(text) => text_string(text),
            Self::Checkbox { .. } => name(self.state().unwrap_or("Off")),
        }
    }
}

fn appearance_stream(dimensions: [i64; 2], data: Vec<u8>, font: Option<&Font>) -> PdfStream {
    let [width, height] = dimensions;
    let resources = font.map_or_else(Dict::new, |font| {
        dictionary([(
            "Font",
            Value::Dictionary(dictionary([(FONT, font.resource())])),
        )])
    });
    PdfStream {
        dict: dictionary([
            ("Type", name("XObject")),
            ("Subtype", name("Form")),
            ("BBox", numbers([0, 0, width, height])),
            ("Resources", Value::Dictionary(resources)),
            ("Length", Value::Integer(data.len() as i64)),
        ]),
        encoded: data.clone(),
        data,
        source_offset: 0,
    }
}

fn appearances(
    form: &Form,
    size: [i64; 2],
    value: &FieldValue,
) -> FieldResult<Vec<(Option<&'static str>, PdfStream)>> {
    let [w, h] = size;
    let background = format!(
        "q 1 g 0 0 {w} {h} re f 0 G 1 w 1 1 {} {} re S\n",
        w - 2,
        h - 2
    );
    match value {
        FieldValue::Text(text) => {
            let font = form.font.as_ref().map_err(|&error| error)?;
            let hex = font.operand(text, size)?;
            let data =
                format!("{background}BT /{FONT} 10 Tf 0 g 2 4 Td <{hex}> Tj ET\nQ\n").into_bytes();
            Ok(vec![(None, appearance_stream(size, data, Some(font)))])
        }
        FieldValue::Checkbox { .. } => {
            let mark = format!(
                "{background}2 w 2 2 m {} {} l 2 {} m {} 2 l S\nQ\n",
                w - 2,
                h - 2,
                h - 2,
                w - 2
            );
            Ok(vec![
                (
                    Some("Off"),
                    appearance_stream(size, format!("{background}Q\n").into_bytes(), None),
                ),
                (None, appearance_stream(size, mark.into_bytes(), None)),
            ])
        }
    }
}

fn stream_matches(graph: &NativePdfGraph, actual: &Value, expected: &PdfStream) -> bool {
    fn same(left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Name(a), Value::Name(b)) => a == b,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Reference(a), Value::Reference(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
            }
            (Value::Dictionary(a), Value::Dictionary(b)) => same_dict(a, b),
            _ => false,
        }
    }
    fn same_dict(a: &Dict, b: &Dict) -> bool {
        a.len() == b.len()
            && b.iter()
                .all(|(key, value)| a.get(key).is_some_and(|entry| same(entry, value)))
    }
    matches!(actual.as_reference().and_then(|reference| graph.objects.get(&reference)),
        Some(Value::Stream(actual)) if actual.data == expected.data && same_dict(&actual.dict, &expected.dict))
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
) -> FieldResult<(Ref, String)> {
    let widget_data = dict(graph, widget).map_err(|_| Reference)?;
    let field = widget_data
        .get("Parent")
        .map_or(Ok(widget), |parent| parent.as_reference().ok_or(Reference))?;
    let fields = dict(graph, field).map_err(|_| Reference)?;
    if fields.contains_key("Parent") && field != widget
        || widget_data.get("P").and_then(Value::as_reference) != Some(page.object)
    {
        return Err(Reference);
    }
    if !matches!(
        fields.get("FT").and_then(Value::as_name),
        Some("Tx" | "Btn")
    ) || fields
        .get("Ff")
        .is_some_and(|flags| !matches!(flags.as_integer(), Some(0 | 2)))
        || field != widget && widget_data.contains_key("T")
    {
        return Err(Profile);
    }
    if pages::annotation_membership(graph, page.object, widget, false).map_err(|_| Reference)?
        != Some(field)
    {
        return Err(Reference);
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
            || !singular(&owner_refs(graph, owner, work)?, container)
        {
            return Err(Reference);
        }
    }
    for object in BTreeSet::from([widget, field]) {
        if form.trailer_refs.contains(&object) {
            return Err(Reference);
        }
        for &owner in graph.reverse.get(&object).ok_or(Reference)? {
            if !singular(&owner_refs(graph, owner, work)?, object) {
                return Err(Reference);
            }
        }
    }
    let field_name = editable_name(fields)
        .filter(|name| form.names.get(name) == Some(&field))
        .ok_or(Reference)?;
    Ok((field, field_name))
}

fn recognize_appearance(
    graph: &NativePdfGraph,
    form: &Form,
    page: &NativePdfNotePage,
    widget: Ref,
    (field, field_name): (Ref, String),
) -> FieldResult<Admitted> {
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
        return Err(Profile);
    }
    let bounds = rect(widget_data.get("Rect").ok_or(Geometry)?).ok_or(Geometry)?;
    let size = dimensions(page, bounds)?;
    let ap = widget_data
        .get("AP")
        .and_then(Value::as_dictionary)
        .filter(|ap| ap.len() == 1)
        .and_then(|ap| ap.get("N"))
        .ok_or(Appearance)?;
    let get_text = |key: &str| {
        fields
            .get(key)
            .and_then(Value::as_bytes)
            .and_then(|bytes| decode(bytes, MAX_TEXT))
            .filter(|value| valid_text(value))
            .ok_or(Profile)
    };
    let value = match fields.get("FT").and_then(Value::as_name) {
        Some("Tx") => {
            if widget_data.contains_key("AS")
                || fields.get("DA").is_some_and(|v| v.as_bytes() != Some(DA))
            {
                return Err(Appearance);
            }
            if fields.contains_key("DV") {
                form.font
                    .as_ref()
                    .map_err(|&error| error)?
                    .operand(&get_text("DV")?, size)?;
            }
            FieldValue::Text(get_text("V")?)
        }
        Some("Btn") => {
            let states = ap
                .as_dictionary()
                .filter(|states| states.len() == 2 && states.contains_key("Off"))
                .ok_or(Appearance)?;
            let on = states
                .keys()
                .find(|key| key.as_str() != "Off")
                .filter(|key| valid_name(key))
                .ok_or(Appearance)?;
            let state = fields.get("V").and_then(Value::as_name).ok_or(Profile)?;
            if !states.contains_key(state)
                || widget_data.get("AS").and_then(Value::as_name) != Some(state)
                || fields
                    .get("DV")
                    .is_some_and(|v| !v.as_name().is_some_and(|v| states.contains_key(v)))
            {
                return Err(Appearance);
            }
            FieldValue::Checkbox {
                checked: state == on,
                on: on.clone(),
            }
        }
        _ => return Err(Profile),
    };
    for (state, stream) in appearances(form, size, &value)? {
        let actual = match &value {
            FieldValue::Text(_) => ap,
            FieldValue::Checkbox { on, .. } => ap
                .as_dictionary()
                .and_then(|states| states.get(state.unwrap_or(on)))
                .ok_or(Appearance)?,
        };
        if !stream_matches(graph, actual, &stream) {
            return Err(Appearance);
        }
    }
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

fn display_name(
    graph: &NativePdfGraph,
    widget: Ref,
    data: &Dict,
    work: &mut usize,
) -> Option<String> {
    let widget_label = data
        .get("T")
        .map(|label| graph.resolve_metered(label, work))
        .transpose()
        .ok()?
        .filter(|label| !matches!(label, Value::Null));
    let label = if let Some(label) = widget_label {
        label
    } else {
        let parent = data.get("Parent")?.as_reference()?;
        let field = dict(graph, parent).ok()?;
        let children = graph
            .resolve_metered(field.get("Kids")?, work)
            .ok()?
            .as_array()?;
        spend(work, children.len()).ok()?;
        if children
            .iter()
            .filter(|child| child.as_reference() == Some(widget))
            .take(2)
            .count()
            != 1
        {
            return None;
        }
        graph.resolve_metered(field.get("T")?, work).ok()?
    };
    decode_field_name(label)
}

fn decode_field_name(label: &Value) -> Option<String> {
    let bytes = label.as_bytes()?;
    let name = if bytes.starts_with(&[0xfe, 0xff]) || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        decode(bytes, MAX_NAME)?
    } else {
        if bytes.len() > MAX_NAME {
            return None;
        }
        decode_pdf_doc_encoding(bytes).ok()?
    };
    (!name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control))
        .then_some(name)
}

pub(super) fn project(
    graph: &NativePdfGraph,
    pages: &[NativePdfNotePage],
    binding: NativeBinding,
    remaining: &mut usize,
) -> Result<Vec<NativeFieldView>, NativeEditError> {
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
    let form = load_form(graph, &mut work);
    let mut result = Vec::new();
    for (page, object, data) in widgets {
        spend(remaining, std::mem::size_of::<NativeFieldView>() * 2)?;
        spend(&mut work, 1)?;
        if let Ok(form) = &form {
            spend(&mut work, form.fields.len() + page.annotations.len())?;
            spend(&mut work, data.get("AP").map_or(0, |_| MAX_TEXT))?;
        }
        let admitted = form.as_ref().map_err(|&error| error).and_then(|form| {
            let identity = recognize_identity(graph, form, page, object, &mut work)?;
            recognize_appearance(graph, form, page, object, identity)
        });
        let readable_name = admitted
            .as_ref()
            .ok()
            .map(|field| field.name.clone())
            .or_else(|| display_name(graph, object, data, &mut work));
        if let Some(name) = &readable_name {
            spend(remaining, name.len())?;
        }
        let mut view = NativeFieldView {
            id: id(binding, object),
            object,
            page: page.id.0,
            kind: None,
            name: readable_name,
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
            spend(remaining, MAX_TEXT)?;
            view.kind = Some(admitted.value.kind());
            match admitted.value {
                FieldValue::Text(value) => view.value = Some(value),
                FieldValue::Checkbox { checked, .. } => view.checked = Some(checked),
            }
        }
        result.push(view);
    }
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
    if let Some(slot) = dict(graph, page)?.get("Annots") {
        if pages::exclusive_container(graph, page, slot)?
            .as_array()
            .is_none()
        {
            return Err(Reference.into());
        }
    }
    if let Some(container) = dict(graph, page)?
        .get("Annots")
        .and_then(Value::as_reference)
        && (form.trailer_refs.contains(&container)
            || !singular(
                &owner_refs(graph, page, &mut graph.remaining_work())?,
                container,
            ))
    {
        return Err(Reference.into());
    }
    let mut annots = pages::annotations(graph, page)?;
    graph.reserve_page_copy(page)?;
    graph.charge_page_work(annots.len())?;
    if add {
        annots.push(widget);
    } else {
        annots.retain(|&entry| entry != widget);
    }
    let page = dict_mut(graph, page)?;
    if annots.is_empty() {
        page.remove("Annots");
    } else {
        page.insert("Annots".into(), refs(annots));
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
    let mut states = Dict::new();
    let mut normal = None;
    for (state, stream) in appearances(form, size, value)? {
        graph.reserve_replacement_stream(stream.data.len())?;
        let reference = Value::Reference(pages::allocate_value(graph, Value::Stream(stream))?);
        match value {
            FieldValue::Text(_) => normal = Some(reference),
            FieldValue::Checkbox { on, .. } => {
                states.insert(state.unwrap_or(on).into(), reference);
            }
        }
    }
    let field_data = dict_mut(graph, field)?;
    field_data.insert("V".into(), value.pdf_value());
    if initialize {
        field_data.insert("DV".into(), value.pdf_value());
    }
    let data = dict_mut(graph, widget)?;
    data.insert("Rect".into(), numbers(bounds));
    data.insert(
        "AP".into(),
        Value::Dictionary(dictionary([(
            "N",
            normal.unwrap_or(Value::Dictionary(states)),
        )])),
    );
    if let Some(state) = value.state() {
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
    use NativeFieldCommand::*;
    command.validate()?;
    let budget = graph.remaining_work();
    let mut work = budget;
    let mut form = load_form(graph, &mut work)?;
    graph.charge_page_work(budget - work)?;
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
        graph.charge_page_work(old_page.annotations.len() + form.fields.len())?;
        let mut work = graph.remaining_work();
        let budget = work;
        let identity = recognize_identity(graph, &form, old_page, view.object, &mut work)?;
        let admitted = recognize_appearance(graph, &form, old_page, view.object, identity)?;
        graph.charge_page_work(budget - work)?;
        graph.reserve_page_copy(admitted.field)?;
        if admitted.field != view.object {
            graph.reserve_page_copy(view.object)?;
        }
        let (page, bounds, value) = match (command, admitted.value) {
            (RemoveField { .. }, _) => {
                replace_annots(graph, &form, old_page.object, view.object, false)?;
                form.fields.retain(|&entry| entry != admitted.field);
                pages::replace_form_fields(graph, form.owner.ok_or(Reference)?, form.fields)?;
                graph.refresh_page_graph(&BTreeSet::from([admitted.field, view.object]))?;
                return Ok(ExpectedField {
                    read_only_name_count: read_only_name_count(current, &admitted.name),
                    name: admitted.name,
                    page: view.page,
                    rect: admitted.rect,
                    value: None,
                });
            }
            (SetFieldText { value, .. }, FieldValue::Text(_)) => {
                (view.page, admitted.rect, FieldValue::Text(value.clone()))
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
                if matches!(value, FieldValue::Text(_)) {
                    if let Some(default) = dict(graph, admitted.field)?.get("DV") {
                        let default = default
                            .as_bytes()
                            .and_then(|bytes| decode(bytes, MAX_TEXT))
                            .ok_or(Profile)?;
                        form.font
                            .as_ref()
                            .map_err(|&reason| reason)?
                            .operand(&default, size)?;
                    }
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
        let (page, rect, field_name, value) = match command {
            AddTextField {
                page,
                rect,
                name,
                value,
            } => (*page, *rect, name, FieldValue::Text(value.clone())),
            AddCheckboxField {
                page,
                rect,
                name,
                checked,
            } => (
                *page,
                *rect,
                name,
                FieldValue::Checkbox {
                    checked: *checked,
                    on: "Yes".into(),
                },
            ),
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
        pages::replace_form_fields(graph, owner, form.fields.clone())?;
        replace_annots(graph, &form, destination.object, widget, true)?;
        (widget, widget, page, field_name.clone(), rect, value)
    };

    if value.kind() == NativeFieldKind::Text {
        let font = form.font.as_ref().map_err(|&reason| reason)?;
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
