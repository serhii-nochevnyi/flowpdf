use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::NativePdfNotePage;
use super::graph::{NativeEditError, NativePdfGraph, spend};
use super::session::NativeSessionError;

type Dict = BTreeMap<String, PdfValue>;
type Ref = PdfObjectRef;
use NativeEditError::{Budget, Malformed, Unsupported};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePageTarget {
    pub page: u32,
    pub stable_id: u64,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativePageSide {
    Before,
    After,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NativePageCommand {
    InsertBlankPage {
        target: NativePageTarget,
        side: NativePageSide,
    },
    ReorderPage {
        target: NativePageTarget,
        position: u32,
    },
    RotatePage {
        target: NativePageTarget,
        degrees: i16,
    },
    DuplicatePage {
        target: NativePageTarget,
    },
    DeletePage {
        target: NativePageTarget,
    },
}

impl NativePageCommand {
    pub(super) fn target(self) -> NativePageTarget {
        match self {
            Self::InsertBlankPage { target, .. }
            | Self::ReorderPage { target, .. }
            | Self::RotatePage { target, .. }
            | Self::DuplicatePage { target }
            | Self::DeletePage { target } => target,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePageState {
    pub order: Vec<u64>,
    pub selected: Option<u64>,
    pub can_delete: bool,
    pub max_position: u32,
}

impl NativePageState {
    pub fn valid_for_count(&self, count: usize) -> bool {
        count > 0
            && self.order.len() == count
            && self.max_position as usize == count - 1
            && self.can_delete == (count > 1)
            && self.order.iter().all(|id| (1..(1 << 53)).contains(id))
            && self.order.iter().copied().collect::<BTreeSet<_>>().len() == count
            && self.selected.is_none_or(|id| self.order.contains(&id))
    }
}

pub(super) fn dict(graph: &NativePdfGraph, reference: Ref) -> Result<&Dict, NativeEditError> {
    graph
        .objects
        .get(&reference)
        .and_then(PdfValue::as_dictionary)
        .ok_or(Malformed)
}

pub(super) fn dict_mut(
    graph: &mut NativePdfGraph,
    reference: Ref,
) -> Result<&mut Dict, NativeEditError> {
    match graph.objects.get_mut(&reference) {
        Some(PdfValue::Dictionary(value)) => Ok(value),
        _ => Err(Malformed),
    }
}

fn reference(dict: &Dict, key: &str) -> Result<Ref, NativeEditError> {
    dict.get(key)
        .and_then(PdfValue::as_reference)
        .ok_or(Malformed)
}

fn array_refs(
    graph: &NativePdfGraph,
    value: &PdfValue,
    work: &mut usize,
) -> Result<Vec<Ref>, NativeEditError> {
    let array = graph
        .resolve_metered(value, work)?
        .as_array()
        .ok_or(Malformed)?;
    spend(work, array.len())?;
    array
        .iter()
        .map(|value| value.as_reference().ok_or(Unsupported))
        .collect()
}

pub(super) fn refs(values: impl IntoIterator<Item = Ref>) -> PdfValue {
    PdfValue::Array(values.into_iter().map(PdfValue::Reference).collect())
}

pub(super) fn allocate(graph: &mut NativePdfGraph, value: Dict) -> Result<Ref, NativeEditError> {
    allocate_value(graph, PdfValue::Dictionary(value))
}

pub(super) fn allocate_value(
    graph: &mut NativePdfGraph,
    value: PdfValue,
) -> Result<Ref, NativeEditError> {
    if graph.objects.len() >= graph.limits.max_objects {
        return Err(Budget);
    }
    let number = graph
        .objects
        .last_key_value()
        .map_or(0, |(key, _)| key.object_number);
    let key = Ref {
        object_number: number.checked_add(1).ok_or(Budget)?,
        generation: 0,
    };
    graph.objects.insert(key, value);
    Ok(key)
}

pub(super) fn catalog(graph: &NativePdfGraph) -> Result<Ref, NativeEditError> {
    reference(&graph.trailer, "Root")
}

fn effective_attributes(graph: &mut NativePdfGraph, page: Ref) -> Result<Dict, NativeEditError> {
    let mut node = page;
    let mut work = graph.remaining_work();
    let mut effective = Dict::new();
    loop {
        spend(&mut work, 1)?;
        let keys: Vec<_> = ["MediaBox", "CropBox", "Resources", "Rotate"]
            .into_iter()
            .filter(|key| {
                !effective.contains_key(*key)
                    && dict(graph, node).is_ok_and(|values| values.contains_key(*key))
            })
            .collect();
        if !keys.is_empty() {
            graph.reserve_page_copy(node)?;
            for key in keys {
                effective.insert(key.into(), dict(graph, node)?[key].clone());
            }
        }
        match dict(graph, node)?.get("Parent") {
            Some(parent) => node = parent.as_reference().ok_or(Malformed)?,
            None => break,
        }
    }
    let media = effective.get("MediaBox").ok_or(Malformed)?;
    if !effective.contains_key("CropBox") {
        effective.insert("CropBox".into(), media.clone());
    }
    effective
        .entry("Resources".into())
        .or_insert_with(|| PdfValue::Dictionary(Dict::new()));
    effective
        .entry("Rotate".into())
        .or_insert(PdfValue::Integer(0));
    Ok(effective)
}

fn change_count(
    graph: &mut NativePdfGraph,
    mut node: Ref,
    delta: i64,
) -> Result<(), NativeEditError> {
    loop {
        let values = dict_mut(graph, node)?;
        let count = values
            .get("Count")
            .and_then(PdfValue::as_integer)
            .ok_or(Malformed)?;
        values.insert(
            "Count".into(),
            PdfValue::Integer(count.checked_add(delta).ok_or(Budget)?),
        );
        match values.get("Parent") {
            Some(parent) => node = parent.as_reference().ok_or(Malformed)?,
            None => return Ok(()),
        }
    }
}

fn kids(graph: &NativePdfGraph, node: Ref) -> Result<Vec<Ref>, NativeEditError> {
    array_refs(
        graph,
        dict(graph, node)?.get("Kids").ok_or(Malformed)?,
        &mut graph.remaining_work(),
    )
}

fn replace_kids(
    graph: &mut NativePdfGraph,
    node: Ref,
    children: Vec<Ref>,
) -> Result<(), NativeEditError> {
    graph.reserve_page_copy(node)?;
    dict_mut(graph, node)?.insert("Kids".into(), refs(children));
    Ok(())
}

pub(super) fn annotations(graph: &NativePdfGraph, page: Ref) -> Result<Vec<Ref>, NativeEditError> {
    dict(graph, page)?
        .get("Annots")
        .map_or(Ok(Vec::new()), |annots| {
            array_refs(graph, annots, &mut graph.remaining_work())
        })
}

fn exclusive_array_member(
    graph: &NativePdfGraph,
    owner: Ref,
    entry: &PdfValue,
    member: Ref,
    extra_owner: Option<Ref>,
) -> Result<(), NativeEditError> {
    let container = entry.as_reference().unwrap_or(owner);
    if container != owner
        && !graph
            .reverse
            .get(&container)
            .is_some_and(|owners| owners.len() == 1 && owners.contains(&owner))
    {
        return Err(Unsupported);
    }
    if !graph.reverse.get(&member).is_some_and(|owners| {
        owners.contains(&container)
            && owners
                .iter()
                .all(|&from| from == container || Some(from) == extra_owner)
    }) {
        return Err(Unsupported);
    }
    Ok(())
}

pub(super) fn exclusive_container<'a>(
    graph: &'a NativePdfGraph,
    owner: Ref,
    value: &'a PdfValue,
) -> Result<&'a PdfValue, NativeEditError> {
    let Some(reference) = value.as_reference() else {
        return Ok(value);
    };
    if !graph
        .reverse
        .get(&reference)
        .is_some_and(|owners| owners.len() == 1 && owners.contains(&owner))
    {
        return Err(Unsupported);
    }
    graph.objects.get(&reference).ok_or(Malformed)
}

pub(super) fn form_dict(graph: &NativePdfGraph, owner: Ref) -> Result<&Dict, NativeEditError> {
    let values = dict(graph, owner)?;
    if owner == catalog(graph)? {
        return values
            .get("AcroForm")
            .and_then(PdfValue::as_dictionary)
            .ok_or(Unsupported);
    }
    Ok(values)
}

pub(super) fn form_fields(graph: &NativePdfGraph) -> Result<(Ref, Vec<Ref>), NativeEditError> {
    let catalog = catalog(graph)?;
    let form = dict(graph, catalog)?.get("AcroForm").ok_or(Unsupported)?;
    let owner = form.as_reference().unwrap_or(catalog);
    exclusive_container(graph, catalog, form)?;
    let fields = form_dict(graph, owner)?.get("Fields").ok_or(Unsupported)?;
    let array = exclusive_container(graph, owner, fields)?
        .as_array()
        .ok_or(Unsupported)?;
    if array.len() > super::forms::MAX_FIELDS {
        return Err(Budget);
    }
    Ok((
        owner,
        array_refs(graph, fields, &mut graph.remaining_work())?,
    ))
}

pub(super) fn replace_form_fields(
    graph: &mut NativePdfGraph,
    owner: Ref,
    fields: Vec<Ref>,
) -> Result<(), NativeEditError> {
    graph.reserve_page_copy(owner)?;
    let direct = owner == catalog(graph)?;
    let values = dict_mut(graph, owner)?;
    let form = if direct {
        match values.get_mut("AcroForm") {
            Some(PdfValue::Dictionary(form)) => form,
            _ => return Err(Malformed),
        }
    } else {
        values
    };
    form.insert("Fields".into(), refs(fields));
    Ok(())
}

fn widget_field(graph: &NativePdfGraph, annot: Ref) -> Result<Ref, NativeEditError> {
    let values = dict(graph, annot)?;
    let field = values
        .get("Parent")
        .map_or(Ok(annot), |parent| parent.as_reference().ok_or(Unsupported))?;
    let field_values = dict(graph, field)?;
    let flags = field_values
        .get("Ff")
        .map_or(Some(0), PdfValue::as_integer)
        .ok_or(Unsupported)?;
    let supported = matches!(
        field_values.get("FT").and_then(PdfValue::as_name),
        Some("Tx" | "Btn")
    ) && flags & !0b111 == 0;
    if !supported
        || field_values.contains_key("Parent")
        || !matches!(field_values.get("T"), Some(PdfValue::String(name)) if !name.is_empty())
        || field_values.keys().any(|key| {
            field != annot
                && ![
                    "FT", "T", "TU", "TM", "Ff", "V", "DV", "DA", "Q", "MaxLen", "Kids",
                ]
                .contains(&key.as_str())
        })
        || field_values.contains_key("AA")
        || values.contains_key("AA")
        || values.contains_key("A")
        || values.contains_key("StructParent")
    {
        return Err(Unsupported);
    }
    match field_values.get("Kids") {
        None if field == annot => {}
        Some(children) if field != annot => {
            let children = exclusive_container(graph, field, children)?
                .as_array()
                .ok_or(Unsupported)?;
            if !matches!(children, [PdfValue::Reference(child)] if *child == annot) {
                return Err(Unsupported);
            }
        }
        _ => return Err(Unsupported),
    }
    Ok(field)
}

pub(super) fn annotation_ownership(
    graph: &NativePdfGraph,
    page: Ref,
    annot: Ref,
) -> Result<Option<Ref>, NativeEditError> {
    annotation_membership(graph, page, annot, true)
}

pub(super) fn annotation_membership(
    graph: &NativePdfGraph,
    page: Ref,
    annot: Ref,
    for_page_operation: bool,
) -> Result<Option<Ref>, NativeEditError> {
    let values = dict(graph, annot)?;
    if values
        .get("P")
        .is_some_and(|value| value.as_reference() != Some(page))
        || ["Popup", "IRT", "StructParent", "OC", "AA"]
            .iter()
            .any(|key| values.contains_key(*key))
    {
        return Err(Unsupported);
    }
    let subtype = values.get("Subtype").and_then(PdfValue::as_name);
    let common = [
        "Type", "Subtype", "Rect", "Contents", "P", "NM", "M", "F", "AP", "AS", "Border", "BS",
        "C", "Lang",
    ];
    let profile: &[&str] = match subtype {
        Some("Text") => &[
            "Open",
            "Name",
            "T",
            "Subj",
            "CA",
            "CreationDate",
            "State",
            "StateModel",
        ],
        Some("Link") => &["Dest", "H", "QuadPoints"],
        Some("Widget") => &[
            "Parent", "FT", "T", "TU", "TM", "Ff", "V", "DV", "DA", "Q", "MaxLen", "MK", "H",
        ],
        _ => return Err(Unsupported),
    };
    if values
        .keys()
        .any(|key| !common.contains(&key.as_str()) && !profile.contains(&key.as_str()))
    {
        return Err(Unsupported);
    }
    let field = match subtype {
        Some("Text" | "Link") => None,
        Some("Widget") => Some(widget_field(graph, annot)?),
        _ => return Err(Unsupported),
    };
    let extra_owner = match field {
        Some(field) if field != annot => {
            let children = &dict(graph, field)?["Kids"];
            Some(children.as_reference().unwrap_or(field))
        }
        Some(_) => {
            let (form, _) = form_fields(graph)?;
            Some(
                form_dict(graph, form)?["Fields"]
                    .as_reference()
                    .unwrap_or(form),
            )
        }
        None => None,
    };
    let annots = dict(graph, page)?.get("Annots").ok_or(Malformed)?;
    let annots_array = exclusive_container(graph, page, annots)?
        .as_array()
        .ok_or(Unsupported)?;
    if annots_array
        .iter()
        .filter(|entry| entry.as_reference() == Some(annot))
        .count()
        != 1
    {
        return Err(Unsupported);
    }
    exclusive_array_member(graph, page, annots, annot, extra_owner)?;
    if let Some(field) = field {
        let (form, fields) = form_fields(graph)?;
        if fields.iter().filter(|&&entry| entry == field).count() != 1 {
            return Err(Unsupported);
        }
        let values = form_dict(graph, form)?;
        if for_page_operation && ["XFA", "CO"].iter().any(|key| values.contains_key(*key)) {
            return Err(Unsupported);
        }
        let page_container = annots.as_reference().unwrap_or(page);
        exclusive_array_member(
            graph,
            form,
            &values["Fields"],
            field,
            Some(if field == annot {
                page_container
            } else {
                annot
            }),
        )?;
    }
    Ok(field)
}

fn unique_field_name(
    graph: &mut NativePdfGraph,
    field: Ref,
    fields: &[Ref],
) -> Result<PdfValue, NativeEditError> {
    graph.charge_page_work(fields.len())?;
    let mut used = BTreeSet::new();
    for &entry in fields {
        let values = dict(graph, entry)?;
        if values.contains_key("Kids") && values.get("FT").is_none() {
            return Err(Unsupported);
        }
        let Some(PdfValue::String(name)) = values.get("T") else {
            return Err(Unsupported);
        };
        // @contract: Reserve the generated namespace across ASCII and Unicode PDF strings.
        if let Some(units) = name.strip_prefix(&[0xfe, 0xff]) {
            if units.len() % 2 != 0 {
                return Err(Unsupported);
            }
            used.insert(
                String::from_utf16(
                    &units
                        .chunks_exact(2)
                        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| Unsupported)?
                .into_bytes(),
            );
        } else {
            used.insert(
                name.strip_prefix(&[0xef, 0xbb, 0xbf])
                    .unwrap_or(name)
                    .to_vec(),
            );
        }
    }
    for suffix in 0..=fields.len() {
        let name = format!("FlowPDFCopy{}_{}", field.object_number, suffix).into_bytes();
        if !used.contains(&name) {
            return Ok(PdfValue::String(name));
        }
    }
    Err(Budget)
}

fn copy_annotations(
    graph: &mut NativePdfGraph,
    source: Ref,
    page: Ref,
) -> Result<Vec<Ref>, NativeEditError> {
    let mut copies = Vec::new();
    let original = annotations(graph, source)?;
    graph.charge_page_work(original.len().saturating_mul(original.len()))?;
    let mut fields_update = None;
    for annot in original {
        let field = annotation_ownership(graph, source, annot)?;
        graph.reserve_page_copy(annot)?;
        let mut copy = dict(graph, annot)?.clone();
        copy.insert("P".into(), PdfValue::Reference(page));
        copy.remove("NM");
        if let Some(field) = field {
            if fields_update.is_none() {
                fields_update = Some(form_fields(graph)?);
            }
            let (_, fields) = fields_update.as_mut().ok_or(Malformed)?;
            if fields.len() >= super::forms::MAX_FIELDS {
                return Err(Budget);
            }
            let name = unique_field_name(graph, field, fields)?;
            let widget = if field == annot {
                copy.remove("TM");
                copy.insert("T".into(), name);
                let widget = allocate(graph, copy)?;
                fields.push(widget);
                widget
            } else {
                graph.reserve_page_copy(field)?;
                let mut field_copy = dict(graph, field)?.clone();
                field_copy.remove("TM");
                field_copy.insert("T".into(), name);
                let field_copy = allocate(graph, field_copy)?;
                copy.insert("Parent".into(), PdfValue::Reference(field_copy));
                let widget = allocate(graph, copy)?;
                dict_mut(graph, field_copy)?.insert("Kids".into(), refs([widget]));
                fields.push(field_copy);
                widget
            };
            copies.push(widget);
        } else {
            copies.push(allocate(graph, copy)?);
        }
    }
    if let Some((form, fields)) = fields_update {
        replace_form_fields(graph, form, fields)?;
    }
    Ok(copies)
}

fn navigation_target(graph: &NativePdfGraph, values: &Dict) -> Option<Ref> {
    let mut work = graph.remaining_work();
    let destination = values.get("Dest")?;
    let dest = graph
        .resolve_metered(destination, &mut work)
        .ok()?
        .as_array()?;
    let mode = dest.get(1)?.as_name()?;
    let length = match mode {
        "Fit" | "FitB" => 2,
        "FitH" | "FitV" | "FitBH" | "FitBV" => 3,
        "XYZ" => 5,
        "FitR" => 6,
        _ => return None,
    };
    if dest.len() != length
        || !dest[2..].iter().all(|value| {
            value.as_number().is_some() || (mode != "FitR" && matches!(value, PdfValue::Null))
        })
    {
        return None;
    }
    dest.first()?.as_reference()
}

fn remove_page_dependencies(
    graph: &mut NativePdfGraph,
    page: Ref,
    order: &[Ref],
) -> Result<BTreeSet<Ref>, NativeEditError> {
    let mut removed = BTreeSet::from([page]);
    let mut fields_to_remove = BTreeSet::new();
    let mut links = Vec::new();
    let budget = graph.remaining_work();
    let mut work = budget;
    let page_annotations = annotations(graph, page)?;
    graph.charge_page_work(
        page_annotations
            .len()
            .saturating_mul(page_annotations.len()),
    )?;
    for annot in page_annotations {
        if !removed.insert(annot) {
            return Err(Unsupported);
        }
        if let Some(field) = annotation_ownership(graph, page, annot)? {
            fields_to_remove.insert(field);
            removed.insert(field);
        }
    }
    for &other in order.iter().filter(|&&other| other != page) {
        // @contract: Unrelated annotation profiles can remain untouched.
        let Some(entry) = dict(graph, other)?.get("Annots") else {
            continue;
        };
        let items = graph
            .resolve_metered(entry, &mut work)?
            .as_array()
            .ok_or(Malformed)?;
        spend(&mut work, items.len())?;
        if !items.iter().any(|item| {
            item.as_reference().is_some_and(|annot| {
                dict(graph, annot).is_ok_and(|values| {
                    values.get("Subtype").and_then(PdfValue::as_name) == Some("Link")
                        && navigation_target(graph, values) == Some(page)
                })
            })
        }) {
            continue;
        }
        let mut annots = annotations(graph, other)?;
        let mut keep = Vec::new();
        for annot in annots.drain(..) {
            let values = dict(graph, annot)?;
            if values.get("Subtype").and_then(PdfValue::as_name) == Some("Link")
                && navigation_target(graph, values) == Some(page)
            {
                annotation_ownership(graph, other, annot)?;
                if !removed.insert(annot) {
                    return Err(Unsupported);
                }
            } else {
                keep.push(annot);
            }
        }
        links.push((other, keep));
    }
    graph.charge_page_work(budget - work)?;
    for (other, keep) in links {
        graph.reserve_page_copy(other)?;
        dict_mut(graph, other)?.insert("Annots".into(), refs(keep));
    }
    if !fields_to_remove.is_empty() {
        let (form, mut fields) = form_fields(graph)?;
        fields.retain(|field| !fields_to_remove.contains(field));
        replace_form_fields(graph, form, fields)?;
    }
    remove_outline_destinations(graph, page, &mut removed)?;
    Ok(removed)
}

fn page_tree_reorder(
    graph: &mut NativePdfGraph,
    old: &[Ref],
    desired: &[Ref],
    projected: &[NativePdfNotePage],
) -> Result<(), NativeEditError> {
    let mut changes = BTreeMap::<Ref, BTreeMap<Ref, Ref>>::new();
    let sources: BTreeMap<_, _> = old.iter().copied().zip(projected).collect();
    for (&before, &after) in old.iter().zip(desired) {
        if before != after {
            let parent = reference(dict(graph, before)?, "Parent")?;
            if reference(dict(graph, after)?, "Parent")? != parent {
                if sources[&after].geometry.is_none()
                    || dict(graph, after)?.contains_key("StructParents")
                {
                    return Err(Unsupported);
                }
                let attributes = effective_attributes(graph, after)?;
                dict_mut(graph, after)?.extend(attributes);
            }
            changes.entry(parent).or_default().insert(before, after);
        }
    }
    for (parent, replacements) in changes {
        let mut children = kids(graph, parent)?;
        for child in &mut children {
            if let Some(&replacement) = replacements.get(child) {
                *child = replacement;
                dict_mut(graph, replacement)?.insert("Parent".into(), PdfValue::Reference(parent));
            }
        }
        replace_kids(graph, parent, children)?;
    }
    Ok(())
}

pub(super) fn stage(
    graph: &mut NativePdfGraph,
    command: NativePageCommand,
    index: usize,
    projected: &[NativePdfNotePage],
) -> Result<Ref, NativeSessionError> {
    use NativePageCommand::*;
    let mut order = graph.pages.clone();
    let page = order[index];
    let parent = reference(dict(graph, page)?, "Parent")?;
    let geometry = projected[index].geometry;
    if matches!(
        command,
        RotatePage { .. } | InsertBlankPage { .. } | DuplicatePage { .. }
    ) && geometry.is_none()
    {
        return Err(NativeSessionError::TargetUnavailable);
    }
    let mut removed = BTreeSet::new();
    if !matches!(command, RotatePage { .. })
        && dict(graph, catalog(graph)?)?.contains_key("PageLabels")
    {
        return Err(Unsupported.into());
    }
    let changes_positions = match command {
        RotatePage { .. } => false,
        ReorderPage { position, .. } => {
            (position as usize) < order.len() && position as usize != index
        }
        DeletePage { .. } => order.len() > 1,
        InsertBlankPage { .. } | DuplicatePage { .. } => true,
    };
    if changes_positions
        && let Some(preferences) = dict(graph, catalog(graph)?)?.get("ViewerPreferences")
    {
        let mut work = graph.remaining_work();
        match graph.resolve_metered(preferences, &mut work)? {
            PdfValue::Null => {}
            preferences => {
                let preferences = preferences.as_dictionary().ok_or(Unsupported)?;
                if let Some(range) = preferences.get("PrintPageRange")
                    && !matches!(graph.resolve_metered(range, &mut work)?, PdfValue::Null)
                {
                    return Err(Unsupported.into());
                }
            }
        }
    }
    let selected = match command {
        RotatePage { degrees, .. } if matches!(degrees, -90 | 90) => {
            let rotation = geometry
                .ok_or(NativeSessionError::TargetUnavailable)?
                .rotation();
            dict_mut(graph, page)?.insert(
                "Rotate".into(),
                PdfValue::Integer((rotation + i64::from(degrees)).rem_euclid(360)),
            );
            page
        }
        RotatePage { .. } => return Err(Unsupported.into()),
        ReorderPage { position, .. } => {
            let position = position as usize;
            if position >= order.len() {
                return Err(NativeSessionError::TargetUnavailable);
            }
            let old = order.clone();
            order.remove(index);
            order.insert(position, page);
            page_tree_reorder(graph, &old, &order, projected)?;
            page
        }
        InsertBlankPage { .. } | DuplicatePage { .. } => {
            let mut children = kids(graph, parent)?;
            let slot = children
                .iter()
                .position(|&child| child == page)
                .ok_or(Malformed)?;
            let new = if matches!(command, DuplicatePage { .. }) {
                let allowed = [
                    "Type",
                    "Parent",
                    "MediaBox",
                    "CropBox",
                    "Rotate",
                    "Resources",
                    "Contents",
                    "Annots",
                    "BleedBox",
                    "TrimBox",
                    "ArtBox",
                    "Group",
                    "UserUnit",
                ];
                if dict(graph, page)?
                    .keys()
                    .any(|key| !allowed.contains(&key.as_str()))
                {
                    return Err(Unsupported.into());
                }
                graph.reserve_page_copy(page)?;
                let mut copy = dict(graph, page)?.clone();
                copy.remove("Annots");
                let new = allocate(graph, copy)?;
                let annots = copy_annotations(graph, page, new)?;
                if !annots.is_empty() {
                    dict_mut(graph, new)?.insert("Annots".into(), refs(annots));
                }
                new
            } else {
                let mut copy = effective_attributes(graph, page)?;
                copy.insert("Type".into(), PdfValue::Name("Page".into()));
                copy.insert("Parent".into(), PdfValue::Reference(parent));
                copy.insert("Resources".into(), PdfValue::Dictionary(Dict::new()));
                allocate(graph, copy)?
            };
            let offset = usize::from(!matches!(
                command,
                InsertBlankPage {
                    side: NativePageSide::Before,
                    ..
                }
            ));
            children.insert(slot + offset, new);
            order.insert(index + offset, new);
            replace_kids(graph, parent, children)?;
            change_count(graph, parent, 1)?;
            new
        }
        DeletePage { .. } => {
            if order.len() == 1 {
                return Err(NativeSessionError::FinalPage);
            }
            if dict(graph, page)?.contains_key("StructParents") {
                return Err(Unsupported.into());
            }
            removed = remove_page_dependencies(graph, page, &order)?;
            let mut children = kids(graph, parent)?;
            children.retain(|&child| child != page);
            replace_kids(graph, parent, children)?;
            change_count(graph, parent, -1)?;
            order.remove(index);
            order[index.min(order.len() - 1)]
        }
    };
    graph.refresh_page_graph(&removed)?;
    if graph.pages != order {
        return Err(NativeSessionError::Postcondition);
    }
    Ok(selected)
}

fn remove_outline_destinations(
    graph: &mut NativePdfGraph,
    page: Ref,
    removed: &mut BTreeSet<Ref>,
) -> Result<(), NativeEditError> {
    let catalog = catalog(graph)?;
    let Some(root) = dict(graph, catalog)?
        .get("Outlines")
        .and_then(PdfValue::as_reference)
    else {
        return Ok(());
    };
    let budget = graph.remaining_work();
    let mut work = budget;
    let mut pending = vec![(root, 0)];
    let mut seen = BTreeSet::from([root]);
    let mut groups = Vec::new();
    while let Some((parent, depth)) = pending.pop() {
        if depth > graph.limits.max_nesting {
            return Err(Unsupported);
        }
        let values = dict(graph, parent)?;
        let last = values.get("Last").and_then(PdfValue::as_reference);
        let mut current = values.get("First").and_then(PdfValue::as_reference);
        let mut previous = None;
        let mut children = Vec::new();
        let mut drops = Vec::new();
        while let Some(child) = current {
            spend(&mut work, 1)?;
            if !seen.insert(child) {
                return Err(Unsupported);
            }
            let values = dict(graph, child)?;
            if reference(values, "Parent")? != parent
                || values.get("Prev").and_then(PdfValue::as_reference) != previous
            {
                return Err(Unsupported);
            }
            current = values.get("Next").and_then(PdfValue::as_reference);
            let branch = values.contains_key("First") || values.contains_key("Last");
            if navigation_target(graph, values) == Some(page) {
                if branch
                    || values
                        .get("Count")
                        .and_then(PdfValue::as_integer)
                        .unwrap_or(0)
                        != 0
                {
                    return Err(Unsupported);
                }
                let mut owners = BTreeSet::new();
                owners.extend(previous);
                owners.extend(current);
                if previous.is_none() || current.is_none() {
                    owners.insert(parent);
                }
                if graph.reverse.get(&child) != Some(&owners) {
                    return Err(Unsupported);
                }
                drops.push(child);
            } else {
                children.push(child);
                if branch {
                    pending.push((child, depth + 1));
                }
            }
            previous = Some(child);
        }
        if previous != last {
            return Err(Unsupported);
        }
        if !drops.is_empty() {
            groups.push((parent, children, drops));
        }
    }
    graph.charge_page_work(budget - work)?;
    for (parent, children, drops) in groups {
        let values = dict_mut(graph, parent)?;
        set_ref(values, "First", children.first().copied());
        set_ref(values, "Last", children.last().copied());
        for (index, &child) in children.iter().enumerate() {
            let values = dict_mut(graph, child)?;
            set_ref(
                values,
                "Prev",
                index.checked_sub(1).map(|index| children[index]),
            );
            set_ref(values, "Next", children.get(index + 1).copied());
        }
        let delta = i64::try_from(drops.len()).map_err(|_| Budget)?;
        let mut ancestor = parent;
        loop {
            let values = dict_mut(graph, ancestor)?;
            let Some(count) = values.get("Count").and_then(PdfValue::as_integer) else {
                if ancestor != root {
                    // @contract: Without a visibility count, ancestor counts are ambiguous.
                    return Err(Unsupported);
                }
                break;
            };
            let magnitude = count
                .checked_abs()
                .and_then(|count| count.checked_sub(delta))
                .filter(|&count| count >= 0)
                .ok_or(Unsupported)?;
            values.insert(
                "Count".into(),
                PdfValue::Integer(if count < 0 { -magnitude } else { magnitude }),
            );
            if ancestor == root || count < 0 {
                break;
            }
            ancestor = reference(values, "Parent")?;
        }
        removed.extend(drops);
    }
    Ok(())
}

fn set_ref(dict: &mut Dict, key: &str, target: Option<Ref>) {
    if let Some(target) = target {
        dict.insert(key.into(), PdfValue::Reference(target));
    } else {
        dict.remove(key);
    }
}
