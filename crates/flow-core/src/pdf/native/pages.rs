use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::NativePdfNotePage;
use super::forms::{MAX_FIELDS, field_name_inventory};
use super::graph::{NativeEditError, NativePdfGraph, PageTreeEdges, collect_refs, spend};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restrictions: Option<NativePageRestrictions>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativePageRefusal {
    Geometry,
    Structure,
}

impl From<NativePageRefusal> for NativeSessionError {
    fn from(refusal: NativePageRefusal) -> Self {
        match refusal {
            NativePageRefusal::Geometry => Self::PageGeometry,
            NativePageRefusal::Structure => Unsupported.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePageRestrictions {
    pub positions_locked: bool,
    pub pages: Vec<NativePageCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePageCapability {
    pub parent: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<NativePageRefusal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reparent: Option<NativePageRefusal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate: Option<NativePageRefusal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<NativePageRefusal>,
}

impl NativePageState {
    pub fn valid_for_count(&self, count: usize) -> bool {
        count > 0
            && self.order.len() == count
            && self
                .restrictions
                .as_ref()
                .is_none_or(|r| r.pages.len() == count)
            && self.max_position as usize == count - 1
            && self.can_delete == (count > 1)
            && self.order.iter().all(|id| (1..(1 << 53)).contains(id))
            && self.order.iter().copied().collect::<BTreeSet<_>>().len() == count
            && self.selected.is_none_or(|id| self.order.contains(&id))
    }
}

const DUPLICATE_KEYS: &[&str] = &[
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

fn positions_locked(graph: &NativePdfGraph) -> Result<bool, NativeEditError> {
    let values = dict(graph, catalog(graph)?)?;
    if values.contains_key("PageLabels") {
        return Ok(true);
    }
    let Some(preferences) = values.get("ViewerPreferences") else {
        return Ok(false);
    };
    let mut work = graph.remaining_work();
    Ok(match graph.resolve_metered(preferences, &mut work) {
        Ok(PdfValue::Null) => false,
        Ok(PdfValue::Dictionary(preferences)) => {
            preferences.get("PrintPageRange").is_some_and(|range| {
                !matches!(graph.resolve_metered(range, &mut work), Ok(PdfValue::Null))
            })
        }
        _ => true,
    })
}

fn named_destination_pages(
    graph: &NativePdfGraph,
    catalog: &Dict,
    work: &mut usize,
) -> Result<BTreeSet<Ref>, NativeEditError> {
    let mut references = Vec::new();
    let mut temporary = graph.limits.max_retained_bytes / 8;
    if let Some(destinations) = catalog.get("Dests") {
        collect_refs(destinations, &mut references, work, &mut temporary)?;
    }
    if let Some(names) = catalog.get("Names") {
        if let Some(destinations) = graph
            .resolve_metered(names, work)?
            .as_dictionary()
            .and_then(|names| names.get("Dests"))
        {
            collect_refs(destinations, &mut references, work, &mut temporary)?;
        }
    }
    let page_refs: BTreeSet<_> = graph.pages.iter().copied().collect();
    let mut visited = BTreeSet::new();
    let mut targets = BTreeSet::new();
    while let Some(reference) = references.pop() {
        if !visited.insert(reference) {
            continue;
        }
        if page_refs.contains(&reference) {
            targets.insert(reference);
        } else {
            let value = graph.objects.get(&reference).ok_or(Malformed)?;
            collect_refs(value, &mut references, work, &mut temporary)?;
        }
    }
    Ok(targets)
}

fn page_widget_count(
    graph: &NativePdfGraph,
    page: Ref,
    work: &mut usize,
) -> Result<usize, NativeEditError> {
    let items = annotations(graph, page)?;
    let cost_per_annotation = items.len() + MAX_FIELDS + graph.limits.max_nesting;
    spend(work, items.len().saturating_mul(cost_per_annotation))?;
    items.into_iter().try_fold(0, |count, annot| {
        let is_widget = annotation_ownership(graph, page, annot)?.is_some();
        Ok(count + usize::from(is_widget))
    })
}

pub(super) fn restrictions(
    graph: &NativePdfGraph,
    projected: &[NativePdfNotePage],
) -> Result<Option<NativePageRestrictions>, NativeEditError> {
    use NativePageRefusal::{Geometry, Structure};

    let positions_locked = positions_locked(graph)?;
    let catalog = catalog(graph)?;
    let catalog_values = dict(graph, catalog)?;
    let mut work = graph.remaining_work();
    let named_targets = named_destination_pages(graph, catalog_values, &mut work);
    let outlines = outline_index(graph, &mut work);
    if matches!(outlines, Err(Budget)) {
        return Err(Budget);
    }
    let trailer = graph.trailer_references(&mut work)?;
    let incoming_links = incoming_link_index(graph, &trailer, &mut work)?;
    let mut parent_refs = PageParentReferences::new(graph);
    let mut copyable_field_count = None;
    let pages: Vec<_> = projected
        .iter()
        .map(|page| {
            let values = dict(graph, page.object)?;
            let parent = reference(values, "Parent")?;
            let geometry = page.geometry.is_none().then_some(Geometry);
            let tagged = values.contains_key("StructParents");
            let widgets = page_widget_count(graph, page.object, &mut work);
            let annotations_supported = widgets.is_ok();
            let duplicate_annotations = match widgets {
                Ok(0) => true,
                Ok(count) => copyable_field_count
                    .get_or_insert_with(|| {
                        let (_, fields) = form_fields(graph).ok()?;
                        field_name_inventory(graph, &fields, &mut work).ok()?;
                        Some(fields.len())
                    })
                    .is_some_and(|fields| fields + count <= MAX_FIELDS),
                Err(_) => false,
            };
            let catalog_dependency = named_targets.as_ref().map_or(true, |pages| {
                pages.contains(&page.object)
                    || graph
                        .reverse
                        .get(&page.object)
                        .is_some_and(|owners| owners.contains(&catalog))
            });
            let delete = if tagged || catalog_dependency || !annotations_supported {
                Some(Structure)
            } else {
                match outlines
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|outlines| {
                        deletion_preflight(
                            graph,
                            page.object,
                            outlines,
                            &incoming_links,
                            &trailer,
                            &mut parent_refs,
                            &mut work,
                            false,
                        )
                    }) {
                    Ok(_) => None,
                    Err(Budget) => return Err(Budget),
                    Err(_) => Some(Structure),
                }
            };
            Ok(NativePageCapability {
                parent: (u64::from(parent.object_number) << 16) | u64::from(parent.generation),
                geometry,
                reparent: geometry.or(tagged.then_some(Structure)),
                duplicate: geometry.or_else(|| {
                    (!duplicate_annotations
                        || values
                            .keys()
                            .any(|key| !DUPLICATE_KEYS.contains(&key.as_str())))
                    .then_some(Structure)
                }),
                delete,
            })
        })
        .collect::<Result<_, NativeEditError>>()?;
    // @contract: Unrestricted single-parent trees omit the optional restrictions.
    Ok((positions_locked
        || pages.iter().any(|page| {
            page.parent != pages[0].parent
                || page.geometry.is_some()
                || page.reparent.is_some()
                || page.duplicate.is_some()
                || page.delete.is_some()
        }))
    .then_some(NativePageRestrictions {
        positions_locked,
        pages,
    }))
}

impl NativePageRestrictions {
    pub(super) fn refusal(&self, command: NativePageCommand) -> Option<NativePageRefusal> {
        use NativePageCommand::*;
        use NativePageRefusal::Structure;

        let index = command.target().page as usize;
        let source = &self.pages[index];
        if self.positions_locked
            && !matches!(command, RotatePage { .. })
            && !matches!(command, ReorderPage { position, .. } if position as usize == index)
        {
            return Some(Structure);
        }
        match command {
            RotatePage { .. } | InsertBlankPage { .. } => source.geometry,
            DuplicatePage { .. } => source.duplicate,
            DeletePage { .. } => source.delete,
            ReorderPage { position, .. } => {
                let position = position as usize;
                let destination = self.pages.get(position)?;
                if source.parent != destination.parent && source.reparent.is_some() {
                    return source.reparent;
                }
                let start = index.min(position);
                let end = index.max(position);
                let mut crossings = self.pages[start..=end].windows(2);
                let displaced_refusal = |pair: &[NativePageCapability]| {
                    if pair[0].parent == pair[1].parent {
                        None
                    } else {
                        pair[usize::from(position > index)].reparent
                    }
                };
                if position > index {
                    crossings.find_map(displaced_refusal)
                } else {
                    crossings.rev().find_map(displaced_refusal)
                }
            }
        }
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
    if array.len() > MAX_FIELDS {
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
    set_form_fields(dict_mut(graph, owner)?, direct, fields)
}

fn set_form_fields(
    values: &mut Dict,
    direct: bool,
    fields: Vec<Ref>,
) -> Result<(), NativeEditError> {
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

fn annotation_profile(
    graph: &NativePdfGraph,
    page: Ref,
    annot: Ref,
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
    match subtype {
        Some("Text" | "Link") => Ok(None),
        Some("Widget") => widget_field(graph, annot).map(Some),
        _ => Err(Unsupported),
    }
}

pub(super) fn annotation_membership(
    graph: &NativePdfGraph,
    page: Ref,
    annot: Ref,
    for_page_operation: bool,
) -> Result<Option<Ref>, NativeEditError> {
    let field = annotation_profile(graph, page, annot)?;
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
        if for_page_operation {
            if values.contains_key("XFA") {
                return Err(Unsupported);
            }
            if let Some(order) = values.get("CO") {
                let resolved = graph.resolve_metered(order, &mut graph.remaining_work())?;
                if !matches!(resolved, PdfValue::Null)
                    && !resolved.as_array().is_some_and(<[_]>::is_empty)
                {
                    return Err(Unsupported);
                }
            }
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

fn unique_field_name(field: Ref, used: &BTreeMap<String, Ref>) -> Result<String, NativeEditError> {
    for suffix in 0..=used.len() {
        let name = format!("FlowPDFCopy{}_{}", field.object_number, suffix);
        if !used.contains_key(&name) {
            return Ok(name);
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
                let (form, fields) = form_fields(graph)?;
                let budget = graph.remaining_work();
                let mut work = budget;
                let names = field_name_inventory(graph, &fields, &mut work)?;
                graph.charge_page_work(budget - work)?;
                fields_update = Some((form, fields, names));
            }
            let (_, fields, names) = fields_update.as_mut().ok_or(Malformed)?;
            if fields.len() >= MAX_FIELDS {
                return Err(Budget);
            }
            let name = unique_field_name(field, names)?;
            let label = PdfValue::String(name.as_bytes().to_vec());
            let (widget, copied_field) = if field == annot {
                copy.remove("TM");
                copy.insert("T".into(), label);
                let widget = allocate(graph, copy)?;
                (widget, widget)
            } else {
                graph.reserve_page_copy(field)?;
                let mut field_copy = dict(graph, field)?.clone();
                field_copy.remove("TM");
                field_copy.insert("T".into(), label);
                let field_copy = allocate(graph, field_copy)?;
                copy.insert("Parent".into(), PdfValue::Reference(field_copy));
                let widget = allocate(graph, copy)?;
                dict_mut(graph, field_copy)?.insert("Kids".into(), refs([widget]));
                (widget, field_copy)
            };
            fields.push(copied_field);
            names.insert(name, copied_field);
            copies.push(widget);
        } else {
            copies.push(allocate(graph, copy)?);
        }
    }
    if let Some((form, fields, _)) = fields_update {
        replace_form_fields(graph, form, fields)?;
    }
    Ok(copies)
}

fn explicit_destination<'a>(
    graph: &'a NativePdfGraph,
    values: &'a Dict,
    work: &mut usize,
) -> Result<Option<&'a [PdfValue]>, NativeEditError> {
    let Some(destination) = values.get("Dest") else {
        return Ok(None);
    };
    match graph.resolve_metered(destination, work) {
        Ok(value) => Ok(value.as_array()),
        Err(Budget) => Err(Budget),
        Err(_) => Ok(None),
    }
}

fn navigation_target(dest: &[PdfValue]) -> Option<Ref> {
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

type LinkCleanup = BTreeMap<Ref, BTreeSet<Ref>>;

#[derive(Default)]
struct IncomingLinkIndex {
    removable: BTreeMap<Ref, LinkCleanup>,
    blocked: BTreeSet<Ref>,
}

impl IncomingLinkIndex {
    fn cleanup_for(&self, page: Ref) -> Result<Option<&LinkCleanup>, NativeEditError> {
        if self.blocked.contains(&page) {
            return Err(Unsupported);
        }
        Ok(self.removable.get(&page))
    }
}

fn incoming_link_index(
    graph: &NativePdfGraph,
    trailer_refs: &BTreeSet<Ref>,
    work: &mut usize,
) -> Result<IncomingLinkIndex, NativeEditError> {
    let mut index = IncomingLinkIndex::default();
    spend(work, graph.pages.len())?;
    let page_refs: BTreeSet<_> = graph.pages.iter().copied().collect();
    index
        .blocked
        .extend(page_refs.intersection(trailer_refs).copied());
    for &source in &graph.pages {
        let source_values = dict(graph, source)?;
        let Some(annots) = source_values.get("Annots") else {
            continue;
        };
        let items = graph
            .resolve_metered(annots, work)?
            .as_array()
            .ok_or(Malformed)?;
        spend(work, items.len())?;
        let array_editable = annots
            .as_reference()
            .is_none_or(|reference| !trailer_refs.contains(&reference))
            && exclusive_container(graph, source, annots)
                .is_ok_and(|value| value.as_array().is_some())
            && items.iter().all(|item| item.as_reference().is_some());
        let mut retained_refs = None;
        for item in items {
            let values = graph
                .resolve_metered(item, work)?
                .as_dictionary()
                .ok_or(Malformed)?;
            let Some(subtype) = values.get("Subtype") else {
                continue;
            };
            if graph.resolve_metered(subtype, work)?.as_name() != Some("Link") {
                continue;
            }
            let Some(dest) = explicit_destination(graph, values, work)? else {
                continue;
            };
            let Some(target) = dest.first().and_then(PdfValue::as_reference) else {
                continue;
            };
            if target == source || !page_refs.contains(&target) {
                continue;
            }
            let Some(annot) = item.as_reference().filter(|&annot| {
                array_editable
                    && !trailer_refs.contains(&annot)
                    && navigation_target(dest).is_some()
                    && matches!(annotation_profile(graph, source, annot), Ok(None))
                    && exclusive_array_member(graph, source, annots, annot, None).is_ok()
            }) else {
                index.blocked.insert(target);
                continue;
            };
            if retained_refs.is_none() {
                let mut refs = Vec::new();
                let mut remaining = graph.remaining_retained();
                for (key, value) in source_values {
                    if key != "Annots" {
                        collect_refs(value, &mut refs, work, &mut remaining)?;
                    }
                }
                retained_refs = Some(refs.into_iter().collect::<BTreeSet<_>>());
            }
            let retained_refs = retained_refs.as_ref().unwrap();
            if retained_refs.contains(&annot)
                || annots
                    .as_reference()
                    .is_some_and(|container| retained_refs.contains(&container))
            {
                index.blocked.insert(target);
                continue;
            }
            let drops = index.removable.entry(target).or_default().entry(source);
            if !drops.or_default().insert(annot) {
                index.blocked.insert(target);
            }
        }
    }
    Ok(index)
}

struct PageOwnedRemovals {
    removed: BTreeSet<Ref>,
    fields: BTreeSet<Ref>,
}

fn page_owned_removals(
    graph: &NativePdfGraph,
    page: Ref,
    work: &mut usize,
) -> Result<PageOwnedRemovals, NativeEditError> {
    let mut removed = BTreeSet::from([page]);
    let mut fields_to_remove = BTreeSet::new();
    let page_annotations = annotations(graph, page)?;
    spend(
        work,
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
    Ok(PageOwnedRemovals {
        removed,
        fields: fields_to_remove,
    })
}

struct PageDeletion {
    removed: BTreeSet<Ref>,
    replacements: BTreeMap<Ref, Dict>,
    retained: usize,
}

impl PageDeletion {
    fn edit<'a>(
        &'a mut self,
        graph: &NativePdfGraph,
        owner: Ref,
        work: &mut usize,
    ) -> Result<&'a mut Dict, NativeEditError> {
        use std::collections::btree_map::Entry;
        Ok(match self.replacements.entry(owner) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                graph.reserve_page_value(owner, work, &mut self.retained)?;
                entry.insert(dict(graph, owner)?.clone())
            }
        })
    }
}

struct PageParentReferences {
    outside_kids: BTreeMap<Ref, BTreeSet<Ref>>,
    indirect_kids: BTreeMap<Ref, BTreeSet<Ref>>,
    retained: usize,
}

impl PageParentReferences {
    fn new(graph: &NativePdfGraph) -> Self {
        Self {
            outside_kids: BTreeMap::new(),
            indirect_kids: BTreeMap::new(),
            retained: graph.remaining_retained(),
        }
    }

    fn retains_reference(
        &mut self,
        parent: Ref,
        values: &Dict,
        target: Ref,
        work: &mut usize,
    ) -> Result<bool, NativeEditError> {
        use std::collections::btree_map::Entry;

        let refs = match self.outside_kids.entry(parent) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                spend(&mut self.retained, 128)?;
                let mut refs = Vec::new();
                for (key, value) in values {
                    spend(work, 1)?;
                    if key != "Kids" {
                        collect_refs(value, &mut refs, work, &mut self.retained)?;
                    }
                }
                entry.insert(refs.into_iter().collect())
            }
        };
        Ok(refs.contains(&target))
    }

    fn indirect_children(
        &mut self,
        graph: &NativePdfGraph,
        array: Ref,
        work: &mut usize,
    ) -> Result<&BTreeSet<Ref>, NativeEditError> {
        use std::collections::btree_map::Entry;

        Ok(match self.indirect_kids.entry(array) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                spend(&mut self.retained, 128)?;
                let array = graph.objects.get(&array).ok_or(Malformed)?;
                let resolved = graph.resolve_metered(array, work)?;
                let mut children = Vec::new();
                collect_refs(resolved, &mut children, work, &mut self.retained)?;
                entry.insert(children.into_iter().collect())
            }
        })
    }
}

fn deletion_preflight(
    graph: &NativePdfGraph,
    page: Ref,
    outlines: &OutlineIndex,
    links: &IncomingLinkIndex,
    trailer: &BTreeSet<Ref>,
    parent_refs: &mut PageParentReferences,
    work: &mut usize,
    materialize_links: bool,
) -> Result<PageDeletion, NativeEditError> {
    let cleanup = links.cleanup_for(page)?;
    if outlines.blocked.contains(&page) {
        return Err(Unsupported);
    }
    let PageOwnedRemovals { removed, fields } = page_owned_removals(graph, page, work)?;
    let mut plan = PageDeletion {
        removed,
        replacements: BTreeMap::new(),
        retained: graph.remaining_retained(),
    };
    let parent = reference(dict(graph, page)?, "Parent")?;
    let parent_values = dict(graph, parent)?;
    let kids = parent_values.get("Kids").ok_or(Malformed)?;
    if !fields.is_empty() {
        let (form, mut keep) = form_fields(graph)?;
        spend(work, keep.len())?;
        keep.retain(|field| !fields.contains(field));
        let direct = form == catalog(graph)?;
        set_form_fields(plan.edit(graph, form, work)?, direct, keep)?;
    }
    plan_outline_cleanup(graph, outlines, page, &mut plan, work)?;
    let mut removed_link_edges = BTreeSet::new();
    if let Some(cleanup) = cleanup {
        for (&source, drops) in cleanup {
            let annots = dict(graph, source)?.get("Annots").ok_or(Malformed)?;
            if materialize_links || plan.replacements.contains_key(&source) {
                let mut keep = array_refs(graph, annots, work)?;
                keep.retain(|annot| !drops.contains(annot));
                plan.edit(graph, source, work)?
                    .insert("Annots".into(), refs(keep));
            } else {
                // @invariant: The link index admits only edges exclusive to this annotation array.
                let container = annots.as_reference().unwrap_or(source);
                spend(work, drops.len())?;
                spend(&mut plan.retained, drops.len().saturating_mul(64))?;
                removed_link_edges.extend(drops.iter().map(|&annot| (container, annot)));
            }
            plan.removed.extend(drops);
        }
    }
    let mut page_tree_edges = if !plan.replacements.contains_key(&parent) {
        let target = kids.as_reference().unwrap_or(page);
        let removed = (!parent_refs.retains_reference(parent, parent_values, target, work)?)
            .then_some((parent, target));
        let retained_children = if let Some(array) = kids.as_reference() {
            Some((parent, parent_refs.indirect_children(graph, array, work)?))
        } else {
            None
        };
        PageTreeEdges {
            removed,
            retained_children,
            removed_links: None,
        }
    } else {
        let mut children = array_refs(graph, kids, work)?;
        children.retain(|&child| child != page);
        plan.edit(graph, parent, work)?
            .insert("Kids".into(), refs(children));
        PageTreeEdges::default()
    };
    page_tree_edges.removed_links = (!removed_link_edges.is_empty()).then_some(&removed_link_edges);
    graph.check_page_removals(
        &plan.removed,
        &plan.replacements,
        page_tree_edges,
        trailer,
        work,
        plan.retained,
    )?;
    Ok(plan)
}

fn page_tree_reorder(
    graph: &mut NativePdfGraph,
    old: &[Ref],
    desired: &[Ref],
) -> Result<(), NativeEditError> {
    let mut changes = BTreeMap::<Ref, BTreeMap<Ref, Ref>>::new();
    for (&before, &after) in old.iter().zip(desired) {
        if before != after {
            let parent = reference(dict(graph, before)?, "Parent")?;
            if reference(dict(graph, after)?, "Parent")? != parent {
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
    let mut removed = BTreeSet::new();
    if !matches!(command, RotatePage { .. })
        && dict(graph, catalog(graph)?)?.contains_key("PageLabels")
    {
        return Err(Unsupported.into());
    }
    let selected = match command {
        RotatePage { degrees, .. } if matches!(degrees, -90 | 90) => {
            let rotation = projected[index]
                .geometry
                .ok_or(NativeSessionError::PageGeometry)?
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
            page_tree_reorder(graph, &old, &order)?;
            page
        }
        InsertBlankPage { .. } | DuplicatePage { .. } => {
            let mut children = kids(graph, parent)?;
            let slot = children
                .iter()
                .position(|&child| child == page)
                .ok_or(Malformed)?;
            let new = if matches!(command, DuplicatePage { .. }) {
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
            let budget = graph.remaining_work();
            let mut work = budget;
            let outlines = outline_index(graph, &mut work)?;
            let trailer = graph.trailer_references(&mut work)?;
            let links = incoming_link_index(graph, &trailer, &mut work)?;
            let plan = deletion_preflight(
                graph,
                page,
                &outlines,
                &links,
                &trailer,
                &mut PageParentReferences::new(graph),
                &mut work,
                true,
            )?;
            let children = if plan.replacements.contains_key(&parent) {
                None
            } else {
                let mut children = array_refs(
                    graph,
                    dict(graph, parent)?.get("Kids").ok_or(Malformed)?,
                    &mut work,
                )?;
                children.retain(|&child| child != page);
                Some(children)
            };
            graph.charge_page_work(budget - work)?;
            graph.charge_page_retained(graph.remaining_retained() - plan.retained)?;
            removed = plan.removed;
            if let Some(children) = children {
                replace_kids(graph, parent, children)?;
            }
            for (owner, values) in plan.replacements {
                *dict_mut(graph, owner)? = values;
            }
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

#[derive(Default)]
struct OutlineIndex {
    root: Option<Ref>,
    siblings: BTreeMap<Ref, Vec<Ref>>,
    removable: BTreeMap<Ref, BTreeMap<Ref, BTreeSet<usize>>>,
    blocked: BTreeSet<Ref>,
}

impl OutlineIndex {
    fn count_changes(
        &self,
        graph: &NativePdfGraph,
        page: Ref,
        work: &mut usize,
    ) -> Result<BTreeMap<Ref, i64>, NativeEditError> {
        if self.blocked.contains(&page) {
            return Err(Unsupported);
        }
        let mut counts = BTreeMap::new();
        let Some(groups) = self.removable.get(&page) else {
            return Ok(counts);
        };
        let root = self.root.ok_or(Malformed)?;
        for (&parent, drops) in groups {
            let delta = i64::try_from(drops.len()).map_err(|_| Budget)?;
            let mut ancestor = parent;
            loop {
                spend(work, 1)?;
                let values = dict(graph, ancestor)?;
                let count = counts
                    .get(&ancestor)
                    .copied()
                    .or_else(|| values.get("Count").and_then(PdfValue::as_integer));
                let Some(count) = count else {
                    if ancestor != root {
                        return Err(Unsupported);
                    }
                    break;
                };
                let magnitude = count
                    .checked_abs()
                    .and_then(|count| count.checked_sub(delta))
                    .filter(|&count| count >= 0)
                    .ok_or(Unsupported)?;
                counts.insert(ancestor, if count < 0 { -magnitude } else { magnitude });
                if ancestor == root || count < 0 {
                    break;
                }
                ancestor = reference(values, "Parent")?;
            }
        }
        Ok(counts)
    }
}

fn outline_index(
    graph: &NativePdfGraph,
    work: &mut usize,
) -> Result<OutlineIndex, NativeEditError> {
    let mut index = OutlineIndex::default();
    let catalog = catalog(graph)?;
    let Some(root) = dict(graph, catalog)?
        .get("Outlines")
        .and_then(PdfValue::as_reference)
    else {
        return Ok(index);
    };
    index.root = Some(root);
    let mut pending = vec![(root, 0)];
    let mut seen = BTreeSet::from([root]);
    while let Some((parent, depth)) = pending.pop() {
        if depth > graph.limits.max_nesting {
            return Err(Unsupported);
        }
        let values = dict(graph, parent)?;
        let last = values.get("Last").and_then(PdfValue::as_reference);
        let mut current = values.get("First").and_then(PdfValue::as_reference);
        let mut previous = None;
        let mut children = Vec::new();
        let mut has_removable = false;
        while let Some(child) = current {
            spend(work, 1)?;
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
            if let Some(page) =
                explicit_destination(graph, values, work)?.and_then(navigation_target)
            {
                let mut owners = BTreeSet::new();
                owners.extend(previous);
                owners.extend(current);
                if previous.is_none() || current.is_none() {
                    owners.insert(parent);
                }
                let removable = !branch
                    && values
                        .get("Count")
                        .and_then(PdfValue::as_integer)
                        .unwrap_or(0)
                        == 0
                    && graph.reverse.get(&child) == Some(&owners);
                if removable {
                    index
                        .removable
                        .entry(page)
                        .or_default()
                        .entry(parent)
                        .or_default()
                        .insert(children.len());
                    has_removable = true;
                } else {
                    index.blocked.insert(page);
                }
            }
            children.push(child);
            if branch {
                pending.push((child, depth + 1));
            }
            previous = Some(child);
        }
        if previous != last {
            return Err(Unsupported);
        }
        if has_removable {
            index.siblings.insert(parent, children);
        }
    }
    Ok(index)
}

fn plan_outline_cleanup(
    graph: &NativePdfGraph,
    index: &OutlineIndex,
    page: Ref,
    plan: &mut PageDeletion,
    work: &mut usize,
) -> Result<(), NativeEditError> {
    let counts = index.count_changes(graph, page, work)?;
    if let Some(groups) = index.removable.get(&page) {
        for (&parent, drops) in groups {
            let siblings = index.siblings.get(&parent).ok_or(Malformed)?;
            spend(work, drops.len())?;
            let mut positions = drops.iter().copied().peekable();
            while let Some(first) = positions.next() {
                let mut last = first;
                plan.removed.insert(siblings[first]);
                while positions.next_if_eq(&(last + 1)).is_some() {
                    last += 1;
                    plan.removed.insert(siblings[last]);
                }
                let previous = first.checked_sub(1).map(|position| siblings[position]);
                let next = siblings.get(last + 1).copied();
                if let Some(previous) = previous {
                    set_ref(plan.edit(graph, previous, work)?, "Next", next);
                } else {
                    set_ref(plan.edit(graph, parent, work)?, "First", next);
                }
                if let Some(next) = next {
                    set_ref(plan.edit(graph, next, work)?, "Prev", previous);
                } else {
                    set_ref(plan.edit(graph, parent, work)?, "Last", previous);
                }
            }
        }
    }
    for (ancestor, count) in counts {
        plan.edit(graph, ancestor, work)?
            .insert("Count".into(), PdfValue::Integer(count));
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
