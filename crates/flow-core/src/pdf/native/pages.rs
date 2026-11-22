use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use serde::{Deserialize, Serialize};

use super::super::reader::{PdfObjectRef, PdfValue};
use super::super::scene::NativePdfNotePage;
use super::forms::{
    FIELD_NAME_ENTRY_BYTES, MAX_FIELDS, NATIVE_MAX_FIELD_NAME_BYTES, field_name_inventory,
};
use super::graph::{
    NativeEditError, NativePdfGraph, PageTreeEdges, collect_reference_set, spend, visit_refs,
};
use super::session::NativeSessionError;
use super::verifier::{Reservation, Workspace};

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
    pub(super) fn has_shape_for_count(&self, count: usize) -> bool {
        count > 0
            && self.order.len() == count
            && self
                .restrictions
                .as_ref()
                .is_none_or(|r| r.pages.len() == count)
            && self.max_position as usize == count - 1
            && self.can_delete == (count > 1)
            && self.order.iter().all(|id| (1..(1 << 53)).contains(id))
            && self.selected.is_none_or(|id| self.order.contains(&id))
    }

    pub fn valid_for_count(&self, count: usize) -> bool {
        self.has_shape_for_count(count)
            && self.order.iter().copied().collect::<BTreeSet<_>>().len() == count
    }

    pub(super) fn valid_in_workspace(
        &self,
        count: usize,
        workspace: &Workspace,
    ) -> Result<bool, NativeEditError> {
        if !self.has_shape_for_count(count) {
            return Ok(false);
        }
        let mut bytes = workspace.scope();
        let mut seen = BTreeSet::new();
        for &id in &self.order {
            if !bytes.insert(&mut seen, id).map_err(|_| Budget)? {
                return Ok(false);
            }
        }
        Ok(true)
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

fn positions_locked(graph: &NativePdfGraph, work: &mut usize) -> Result<bool, NativeEditError> {
    let values = dict(graph, catalog(graph)?)?;
    if values.contains_key("PageLabels") {
        return Ok(true);
    }
    let Some(preferences) = values.get("ViewerPreferences") else {
        return Ok(false);
    };
    let preferences = match graph.resolve_metered(preferences, work) {
        Ok(PdfValue::Null) => return Ok(false),
        Ok(PdfValue::Dictionary(preferences)) => preferences,
        Err(Budget) => return Err(Budget),
        _ => return Ok(true),
    };
    let Some(range) = preferences.get("PrintPageRange") else {
        return Ok(false);
    };
    match graph.resolve_metered(range, work) {
        Ok(PdfValue::Null) => Ok(false),
        Err(Budget) => Err(Budget),
        _ => Ok(true),
    }
}

fn scoped_entry<'a, K: Ord, V: Default>(
    map: &'a mut BTreeMap<K, V>,
    key: K,
    reservation: &mut Reservation<'_>,
) -> Result<&'a mut V, NativeEditError> {
    use std::collections::btree_map::Entry;

    Ok(match map.entry(key) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => {
            reservation.tree_entry::<(K, V)>().map_err(|_| Budget)?;
            entry.insert(V::default())
        }
    })
}

fn named_destination_pages<'a>(
    graph: &NativePdfGraph,
    catalog: &Dict,
    page_refs: &BTreeSet<Ref>,
    work: &mut usize,
    workspace: &'a Workspace,
) -> Result<(BTreeSet<Ref>, Reservation<'a>), NativeEditError> {
    let mut stack = workspace.scope();
    let mut references = Vec::new();
    let names = if let Some(names) = catalog.get("Names") {
        graph
            .resolve_metered(names, work)?
            .as_dictionary()
            .and_then(|names| names.get("Dests"))
    } else {
        None
    };
    for destinations in catalog.get("Dests").into_iter().chain(names) {
        visit_refs(destinations, work, &mut |reference| {
            stack.push(&mut references, reference).map_err(|_| Budget)
        })?;
    }
    let mut visited_bytes = workspace.scope();
    let mut visited = BTreeSet::new();
    let mut target_bytes = workspace.scope();
    let mut targets = BTreeSet::new();
    while let Some(reference) = references.pop() {
        if !visited_bytes
            .insert(&mut visited, reference)
            .map_err(|_| Budget)?
        {
            continue;
        }
        if page_refs.contains(&reference) {
            target_bytes
                .insert(&mut targets, reference)
                .map_err(|_| Budget)?;
        } else {
            let value = graph.objects.get(&reference).ok_or(Malformed)?;
            visit_refs(value, work, &mut |reference| {
                stack.push(&mut references, reference).map_err(|_| Budget)
            })?;
        }
    }
    Ok((targets, target_bytes))
}

fn page_reference_set(
    graph: &NativePdfGraph,
    work: &mut usize,
    reservation: &mut Reservation<'_>,
) -> Result<BTreeSet<Ref>, NativeEditError> {
    spend(work, graph.pages.len())?;
    let mut pages = BTreeSet::new();
    for &page in &graph.pages {
        reservation.insert(&mut pages, page).map_err(|_| Budget)?;
    }
    Ok(pages)
}

fn page_widget_count(
    graph: &NativePdfGraph,
    page: Ref,
    work: &mut usize,
) -> Result<usize, NativeEditError> {
    let items = annotation_refs(graph, page, work)?;
    items.iter().try_fold(0, |count, annot| {
        let annot = annot.as_reference().ok_or(Unsupported)?;
        let is_widget = annotation_membership(graph, page, annot, true, work)?.is_some();
        Ok(count + usize::from(is_widget))
    })
}

fn copyable_fields(
    graph: &NativePdfGraph,
    work: &mut usize,
    workspace: &Workspace,
) -> Result<Option<usize>, NativeEditError> {
    let (_, values) = match form_field_refs(graph, work, Unsupported) {
        Ok(fields) => fields,
        Err(Budget) => return Err(Budget),
        Err(_) => return Ok(None),
    };
    let _field_bytes = workspace
        .reserve(values.len() * size_of::<Ref>())
        .map_err(|_| Budget)?;
    let mut fields = Vec::new();
    fields.try_reserve_exact(values.len()).map_err(|_| Budget)?;
    fields.extend(values.iter().filter_map(PdfValue::as_reference));
    let mut available = workspace.remaining();
    match field_name_inventory(graph, &fields, work, &mut available) {
        Ok(_) => Ok(Some(fields.len())),
        Err(Budget) => Err(Budget),
        Err(_) => Ok(None),
    }
}

pub(super) fn restrictions(
    graph: &NativePdfGraph,
    projected: &[NativePdfNotePage],
    workspace: &Workspace,
) -> Result<Option<NativePageRestrictions>, NativeEditError> {
    use NativePageRefusal::{Geometry, Structure};

    let mut work = graph.remaining_work();
    let positions_locked = positions_locked(graph, &mut work)?;
    let catalog = catalog(graph)?;
    let catalog_values = dict(graph, catalog)?;
    let mut page_bytes = workspace.scope();
    let page_refs = page_reference_set(graph, &mut work, &mut page_bytes)?;
    let named_targets =
        named_destination_pages(graph, catalog_values, &page_refs, &mut work, workspace);
    if matches!(named_targets, Err(Budget)) {
        return Err(Budget);
    }
    let mut trailer_bytes = workspace.scope();
    let trailer = graph.trailer_references(&mut work, &mut trailer_bytes)?;
    let incoming_links = incoming_link_index(graph, &page_refs, &trailer, &mut work, workspace)?;
    drop(page_refs);
    drop(page_bytes);
    let outlines = outline_index(graph, &mut work, workspace);
    if matches!(outlines, Err(Budget)) {
        return Err(Budget);
    }
    let mut parent_refs = PageParentReferences::new(workspace);
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
                Ok(count) => {
                    if copyable_field_count.is_none() {
                        copyable_field_count = Some(copyable_fields(graph, &mut work, workspace)?);
                    }
                    copyable_field_count
                        .flatten()
                        .is_some_and(|fields| fields + count <= MAX_FIELDS)
                }
                Err(Budget) => return Err(Budget),
                Err(_) => false,
            };
            let catalog_dependency = named_targets.as_ref().map_or(true, |(pages, _)| {
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

fn reference_array<'a>(
    graph: &'a NativePdfGraph,
    value: &'a PdfValue,
    work: &mut usize,
) -> Result<&'a [PdfValue], NativeEditError> {
    let array = graph
        .resolve_metered(value, work)?
        .as_array()
        .ok_or(Malformed)?;
    spend(work, array.len())?;
    Ok(array)
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
        let value = values.get_mut("Count").ok_or(Malformed)?;
        let count = value.as_integer().ok_or(Malformed)?;
        *value = PdfValue::Integer(count.checked_add(delta).ok_or(Budget)?);
        match values.get("Parent") {
            Some(parent) => node = parent.as_reference().ok_or(Malformed)?,
            None => return Ok(()),
        }
    }
}

fn with_page_scratch<T>(
    graph: &mut NativePdfGraph,
    bytes: usize,
    edit: impl FnOnce(&mut NativePdfGraph) -> Result<T, NativeSessionError>,
) -> Result<T, NativeSessionError> {
    graph.charge_page_retained(bytes)?;
    let result = edit(graph);
    graph.release_page_retained(bytes);
    result
}

fn edit_kids<T>(
    graph: &mut NativePdfGraph,
    node: Ref,
    extra: usize,
    edit: impl FnOnce(&mut NativePdfGraph, &mut Vec<Ref>) -> Result<T, NativeSessionError>,
) -> Result<T, NativeSessionError> {
    let budget = graph.remaining_work();
    let mut work = budget;
    let slot = dict(graph, node)?.get("Kids").ok_or(Malformed)?;
    let indirect = slot.as_reference().is_some();
    let source = reference_array(graph, slot, &mut work)?;
    let count = source.len().checked_add(extra).ok_or(Budget)?;
    let bytes = count.checked_mul(size_of::<Ref>()).ok_or(Budget)?;
    let workspace = Workspace::new(graph.remaining_retained());
    let scratch = workspace.reserve(bytes).map_err(|_| Budget)?;
    let mut children = Vec::new();
    children.try_reserve_exact(count).map_err(|_| Budget)?;
    for value in source {
        children.push(value.as_reference().ok_or(Unsupported)?);
    }
    graph.charge_page_work(budget - work)?;
    with_page_scratch(graph, bytes, move |graph| {
        drop(scratch);
        let result = edit(graph, &mut children)?;
        graph.reserve_page_copy(node)?;
        // @invariant: The owner copy covers direct slots, and the graph still owns the old indirect array.
        let new_slots = if indirect { children.len() } else { extra };
        let installed = new_slots.checked_mul(size_of::<PdfValue>()).ok_or(Budget)?;
        graph.charge_page_retained(installed)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(children.len())
            .map_err(|_| Budget)?;
        values.extend(children.into_iter().map(PdfValue::Reference));
        dict_mut(graph, node)?.insert("Kids".into(), PdfValue::Array(values));
        Ok(result)
    })
}

pub(super) fn annotation_refs<'a>(
    graph: &'a NativePdfGraph,
    page: Ref,
    work: &mut usize,
) -> Result<&'a [PdfValue], NativeEditError> {
    dict(graph, page)?
        .get("Annots")
        .map_or(Ok(&[][..]), |annots| reference_array(graph, annots, work))
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

pub(super) fn form_fields(
    graph: &NativePdfGraph,
    work: &mut usize,
    remaining: &mut usize,
) -> Result<(Ref, Vec<Ref>), NativeEditError> {
    let (owner, values) = form_field_refs(graph, work, Unsupported)?;
    let mut available = *remaining;
    spend(&mut available, values.len() * size_of::<Ref>())?;
    let mut fields = Vec::new();
    fields.try_reserve_exact(values.len()).map_err(|_| Budget)?;
    fields.extend(values.iter().filter_map(PdfValue::as_reference));
    *remaining = available;
    Ok((owner, fields))
}

pub(super) fn reserve_form_field_slot(
    graph: &mut NativePdfGraph,
    fields: &mut Vec<Ref>,
) -> Result<usize, NativeEditError> {
    if fields.len() < fields.capacity() {
        return Ok(0);
    }
    let old_bytes = fields.capacity() * size_of::<Ref>();
    let new_bytes = (fields.len() + 1) * size_of::<Ref>();
    graph.charge_page_retained(new_bytes)?;
    if fields.try_reserve_exact(1).is_err() {
        graph.release_page_retained(new_bytes);
        return Err(Budget);
    }
    graph.release_page_retained(old_bytes);
    Ok(new_bytes - old_bytes)
}

fn form_field_refs<'a>(
    graph: &'a NativePdfGraph,
    work: &mut usize,
    field_limit_error: NativeEditError,
) -> Result<(Ref, &'a [PdfValue]), NativeEditError> {
    let catalog = catalog(graph)?;
    let form = dict(graph, catalog)?.get("AcroForm").ok_or(Unsupported)?;
    let owner = form.as_reference().unwrap_or(catalog);
    exclusive_container(graph, catalog, form)?;
    let fields = form_dict(graph, owner)?.get("Fields").ok_or(Unsupported)?;
    let array = exclusive_container(graph, owner, fields)?
        .as_array()
        .ok_or(Unsupported)?;
    if array.len() > MAX_FIELDS {
        return Err(field_limit_error);
    }
    let refs = reference_array(graph, fields, work)?;
    if refs.iter().any(|value| value.as_reference().is_none()) {
        return Err(Unsupported);
    }
    Ok((owner, refs))
}

pub(super) fn replace_form_fields(
    graph: &mut NativePdfGraph,
    owner: Ref,
    fields: Vec<Ref>,
) -> Result<(), NativeEditError> {
    let direct_slots = form_dict(graph, owner)?
        .get("Fields")
        .and_then(PdfValue::as_array)
        .map_or(0, <[_]>::len);
    let installed_bytes = fields
        .len()
        .saturating_sub(direct_slots)
        .checked_mul(size_of::<PdfValue>())
        .ok_or(Budget)?;
    let available = graph.remaining_retained();
    let work_budget = graph.remaining_work();
    let mut remaining = available;
    let mut work = work_budget;
    graph.reserve_page_value(owner, &mut work, &mut remaining)?;
    spend(&mut remaining, installed_bytes)?;
    let retained = available - remaining;
    graph.charge_page_work(work_budget - work)?;
    graph.charge_page_retained(retained)?;
    let mut values = Vec::new();
    if values.try_reserve_exact(fields.len()).is_err() {
        graph.release_page_retained(retained);
        return Err(Budget);
    }
    values.extend(fields.into_iter().map(PdfValue::Reference));
    let direct = owner == catalog(graph)?;
    mutable_form(dict_mut(graph, owner)?, direct)?.insert("Fields".into(), PdfValue::Array(values));
    Ok(())
}

fn mutable_form(values: &mut Dict, direct: bool) -> Result<&mut Dict, NativeEditError> {
    Ok(if direct {
        match values.get_mut("AcroForm") {
            Some(PdfValue::Dictionary(form)) => form,
            _ => return Err(Malformed),
        }
    } else {
        values
    })
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
    work: &mut usize,
) -> Result<Option<Ref>, NativeEditError> {
    let field = annotation_profile(graph, page, annot)?;
    let form = field
        .map(|_| {
            form_field_refs(
                graph,
                work,
                if for_page_operation {
                    Unsupported
                } else {
                    Budget
                },
            )
        })
        .transpose()?;
    let extra_owner = match field {
        Some(field) if field != annot => {
            let children = &dict(graph, field)?["Kids"];
            Some(children.as_reference().unwrap_or(field))
        }
        Some(_) => {
            let (form, _) = form.ok_or(Malformed)?;
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
    spend(work, annots_array.len())?;
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
        let (form, fields) = form.ok_or(Malformed)?;
        spend(work, fields.len())?;
        if fields
            .iter()
            .filter(|entry| entry.as_reference() == Some(field))
            .count()
            != 1
        {
            return Err(Unsupported);
        }
        let values = form_dict(graph, form)?;
        if for_page_operation {
            if values.contains_key("XFA") {
                return Err(Unsupported);
            }
            if let Some(order) = values.get("CO") {
                let resolved = graph.resolve_metered(order, work)?;
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
    let mut name = String::new();
    name.try_reserve_exact(NATIVE_MAX_FIELD_NAME_BYTES)
        .map_err(|_| Budget)?;
    for suffix in 0..=used.len() {
        name.clear();
        write!(name, "FlowPDFCopy{}_{}", field.object_number, suffix).map_err(|_| Budget)?;
        if !used.contains_key(name.as_str()) {
            return Ok(name);
        }
    }
    Err(Budget)
}

fn indirect_array_slot_bytes(slot: &PdfValue, count: usize) -> Result<usize, NativeEditError> {
    // @invariant: The owner copy covers direct slots. Indirect slots are charged to their original array.
    if slot.as_reference().is_some() {
        count.checked_mul(size_of::<PdfValue>()).ok_or(Budget)
    } else {
        Ok(0)
    }
}

fn copy_annotations(
    graph: &mut NativePdfGraph,
    source: Ref,
    page: Ref,
) -> Result<Vec<PdfValue>, NativeSessionError> {
    let Some(slot) = dict(graph, source)?.get("Annots") else {
        return Ok(Vec::new());
    };
    let budget = graph.remaining_work();
    let mut work = budget;
    let values = reference_array(graph, slot, &mut work)?;
    let count = values.len();
    let scratch_bytes = count.checked_mul(size_of::<Ref>()).ok_or(Budget)?;
    let installed_bytes = indirect_array_slot_bytes(slot, count)?;
    let mut remaining = graph.remaining_retained();
    spend(&mut remaining, scratch_bytes)?;
    spend(&mut remaining, installed_bytes)?;
    let mut original = Vec::new();
    original.try_reserve_exact(count).map_err(|_| Budget)?;
    for value in values {
        original.push(value.as_reference().ok_or(Unsupported)?);
    }
    graph.charge_page_work(budget - work)?;
    graph.charge_page_retained(installed_bytes)?;
    with_page_scratch(graph, scratch_bytes, move |graph| {
        Ok(copy_annotation_objects(graph, source, page, original)?)
    })
}

fn copy_annotation_objects(
    graph: &mut NativePdfGraph,
    source: Ref,
    page: Ref,
    original: Vec<Ref>,
) -> Result<Vec<PdfValue>, NativeEditError> {
    let mut copies = Vec::new();
    copies
        .try_reserve_exact(original.len())
        .map_err(|_| Budget)?;
    let mut fields_update = None;
    let mut inventory_charge = 0;
    for annot in original.iter().copied() {
        let budget = graph.remaining_work();
        let mut work = budget;
        let field = annotation_membership(graph, source, annot, true, &mut work)?;
        graph.charge_page_work(budget - work)?;
        graph.reserve_page_copy(annot)?;
        let mut copy = dict(graph, annot)?.clone();
        copy.insert("P".into(), PdfValue::Reference(page));
        copy.remove("NM");
        if let Some(field) = field {
            if fields_update.is_none() {
                let budget = graph.remaining_work();
                let mut work = budget;
                let available = graph.remaining_retained();
                let mut remaining = available;
                let (form, fields) = form_fields(graph, &mut work, &mut remaining)?;
                let names = field_name_inventory(graph, &fields, &mut work, &mut remaining)?;
                graph.charge_page_work(budget - work)?;
                inventory_charge = available - remaining;
                graph.charge_page_retained(inventory_charge)?;
                fields_update = Some((form, fields, names));
            }
            let (_, fields, names) = fields_update.as_mut().ok_or(Malformed)?;
            if fields.len() >= MAX_FIELDS {
                return Err(Budget);
            }
            inventory_charge += reserve_form_field_slot(graph, fields)?;
            let name_entry_charge = FIELD_NAME_ENTRY_BYTES + NATIVE_MAX_FIELD_NAME_BYTES;
            graph.charge_page_retained(name_entry_charge + NATIVE_MAX_FIELD_NAME_BYTES)?;
            inventory_charge += name_entry_charge;
            let name = unique_field_name(field, names)?;
            let label = PdfValue::String(name.as_bytes().to_vec());
            let (widget, copied_field) = if field == annot {
                copy.remove("TM");
                copy.insert("T".into(), label);
                let widget = allocate(graph, copy)?;
                (widget, widget)
            } else {
                graph.reserve_page_copy(field)?;
                let children = dict(graph, field)?.get("Kids").ok_or(Malformed)?;
                let installed_bytes = indirect_array_slot_bytes(children, 1)?;
                graph.charge_page_retained(installed_bytes)?;
                let mut field_copy = dict(graph, field)?.clone();
                field_copy.remove("TM");
                field_copy.remove("Kids");
                field_copy.insert("T".into(), label);
                let field_copy = allocate(graph, field_copy)?;
                copy.insert("Parent".into(), PdfValue::Reference(field_copy));
                let widget = allocate(graph, copy)?;
                let mut children = Vec::new();
                children.try_reserve_exact(1).map_err(|_| Budget)?;
                children.push(PdfValue::Reference(widget));
                dict_mut(graph, field_copy)?.insert("Kids".into(), PdfValue::Array(children));
                (widget, field_copy)
            };
            fields.push(copied_field);
            names.insert(name, copied_field);
            copies.push(PdfValue::Reference(widget));
        } else {
            copies.push(PdfValue::Reference(allocate(graph, copy)?));
        }
    }
    if let Some((form, fields, names)) = fields_update {
        let field_bytes = fields.capacity() * size_of::<Ref>();
        drop(names);
        graph.release_page_retained(inventory_charge - field_bytes);
        let result = replace_form_fields(graph, form, fields);
        graph.release_page_retained(field_bytes);
        result?;
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

struct IncomingLinkIndex<'a> {
    removable: BTreeMap<Ref, LinkCleanup>,
    blocked: BTreeSet<Ref>,
    bytes: Reservation<'a>,
}

impl IncomingLinkIndex<'_> {
    fn cleanup_for(&self, page: Ref) -> Result<Option<&LinkCleanup>, NativeEditError> {
        if self.blocked.contains(&page) {
            return Err(Unsupported);
        }
        Ok(self.removable.get(&page))
    }
}

fn incoming_link_index<'a>(
    graph: &NativePdfGraph,
    page_refs: &BTreeSet<Ref>,
    trailer_refs: &BTreeSet<Ref>,
    work: &mut usize,
    workspace: &'a Workspace,
) -> Result<IncomingLinkIndex<'a>, NativeEditError> {
    let mut index = IncomingLinkIndex {
        removable: BTreeMap::new(),
        blocked: BTreeSet::new(),
        bytes: workspace.scope(),
    };
    for &page in page_refs.intersection(trailer_refs) {
        index
            .bytes
            .insert(&mut index.blocked, page)
            .map_err(|_| Budget)?;
    }
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
        let mut reference_bytes = workspace.scope();
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
                index
                    .bytes
                    .insert(&mut index.blocked, target)
                    .map_err(|_| Budget)?;
                continue;
            };
            if retained_refs.is_none() {
                let mut refs = BTreeSet::new();
                for (key, value) in source_values {
                    if key != "Annots" {
                        collect_reference_set(value, &mut refs, work, &mut reference_bytes)?;
                    }
                }
                retained_refs = Some(refs);
            }
            let retained_refs = retained_refs.as_ref().unwrap();
            if retained_refs.contains(&annot)
                || annots
                    .as_reference()
                    .is_some_and(|container| retained_refs.contains(&container))
            {
                index
                    .bytes
                    .insert(&mut index.blocked, target)
                    .map_err(|_| Budget)?;
                continue;
            }
            let sources = scoped_entry(&mut index.removable, target, &mut index.bytes)?;
            let drops = scoped_entry(sources, source, &mut index.bytes)?;
            if !index.bytes.insert(drops, annot).map_err(|_| Budget)? {
                index
                    .bytes
                    .insert(&mut index.blocked, target)
                    .map_err(|_| Budget)?;
            }
        }
    }
    Ok(index)
}

struct PageDeletion<'a> {
    removed: BTreeSet<Ref>,
    replacements: BTreeMap<Ref, Dict>,
    removal_bytes: Reservation<'a>,
    entry_bytes: Reservation<'a>,
    value_bytes: Reservation<'a>,
    installed_bytes: usize,
}

impl<'a> PageDeletion<'a> {
    fn new(workspace: &'a Workspace) -> Self {
        Self {
            removed: BTreeSet::new(),
            replacements: BTreeMap::new(),
            removal_bytes: workspace.scope(),
            entry_bytes: workspace.scope(),
            value_bytes: workspace.scope(),
            installed_bytes: 0,
        }
    }

    fn remove(&mut self, target: Ref) -> Result<bool, NativeEditError> {
        self.removal_bytes
            .insert(&mut self.removed, target)
            .map_err(|_| Budget)
    }

    fn edit(
        &mut self,
        graph: &NativePdfGraph,
        owner: Ref,
        work: &mut usize,
    ) -> Result<&mut Dict, NativeEditError> {
        use std::collections::btree_map::Entry;
        Ok(match self.replacements.entry(owner) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                self.entry_bytes
                    .tree_entry::<(Ref, Dict)>()
                    .map_err(|_| Budget)?;
                let available = self.value_bytes.workspace().remaining();
                let mut remaining = available;
                graph.reserve_page_value(owner, work, &mut remaining)?;
                let bytes = available - remaining;
                self.value_bytes.grow(bytes).map_err(|_| Budget)?;
                self.installed_bytes += bytes;
                entry.insert(dict(graph, owner)?.clone())
            }
        })
    }

    fn keep_array_members(
        &mut self,
        graph: &NativePdfGraph,
        owner: Ref,
        key: &str,
        direct_form: bool,
        keep: impl Fn(Ref) -> bool,
        work: &mut usize,
    ) -> Result<(), NativeEditError> {
        let values = if direct_form {
            form_dict(graph, owner)?
        } else {
            dict(graph, owner)?
        };
        let original = values.get(key).ok_or(Malformed)?;
        let source = reference_array(graph, original, work)?;
        if source.iter().any(|value| value.as_reference().is_none()) {
            return Err(Unsupported);
        }
        let indirect = if original.as_reference().is_some() {
            let retained = source
                .iter()
                .filter_map(PdfValue::as_reference)
                .filter(|&item| keep(item));
            spend(work, source.len())?;
            let count = retained.clone().count();
            let bytes = count * size_of::<PdfValue>();
            self.value_bytes.grow(bytes).map_err(|_| Budget)?;
            self.installed_bytes += bytes;
            let mut values = Vec::new();
            values.try_reserve_exact(count).map_err(|_| Budget)?;
            values.extend(retained.map(PdfValue::Reference));
            Some(values)
        } else {
            None
        };
        let values = self.edit(graph, owner, work)?;
        let array = mutable_form(values, direct_form)?
            .get_mut(key)
            .ok_or(Malformed)?;
        if let Some(indirect) = indirect {
            *array = PdfValue::Array(indirect);
        } else if let PdfValue::Array(items) = array {
            items.retain(|item| item.as_reference().is_some_and(&keep));
        } else {
            return Err(Malformed);
        }
        Ok(())
    }
}

struct PageParentReferences<'a> {
    outside_kids: BTreeMap<Ref, BTreeSet<Ref>>,
    indirect_kids: BTreeMap<Ref, BTreeSet<Ref>>,
    bytes: Reservation<'a>,
}

impl<'a> PageParentReferences<'a> {
    fn new(workspace: &'a Workspace) -> Self {
        Self {
            outside_kids: BTreeMap::new(),
            indirect_kids: BTreeMap::new(),
            bytes: workspace.scope(),
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
                self.bytes
                    .tree_entry::<(Ref, BTreeSet<Ref>)>()
                    .map_err(|_| Budget)?;
                let mut refs = BTreeSet::new();
                for (key, value) in values {
                    spend(work, 1)?;
                    if key != "Kids" {
                        collect_reference_set(value, &mut refs, work, &mut self.bytes)?;
                    }
                }
                entry.insert(refs)
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
                let array = graph.objects.get(&array).ok_or(Malformed)?;
                let resolved = graph.resolve_metered(array, work)?;
                self.bytes
                    .tree_entry::<(Ref, BTreeSet<Ref>)>()
                    .map_err(|_| Budget)?;
                let mut children = BTreeSet::new();
                collect_reference_set(resolved, &mut children, work, &mut self.bytes)?;
                entry.insert(children)
            }
        })
    }
}

fn deletion_preflight<'a>(
    graph: &NativePdfGraph,
    page: Ref,
    outlines: &OutlineIndex<'_>,
    links: &IncomingLinkIndex<'_>,
    trailer: &BTreeSet<Ref>,
    parent_refs: &mut PageParentReferences<'a>,
    work: &mut usize,
    materialize_links: bool,
) -> Result<PageDeletion<'a>, NativeEditError> {
    let cleanup = links.cleanup_for(page)?;
    if outlines.blocked.contains(&page) {
        return Err(Unsupported);
    }
    let workspace = parent_refs.bytes.workspace();
    let mut plan = PageDeletion::new(workspace);
    plan.remove(page)?;
    {
        let mut field_bytes = workspace.scope();
        let mut fields = BTreeSet::new();
        let annotations = annotation_refs(graph, page, work)?;
        for annot in annotations {
            let annot = annot.as_reference().ok_or(Unsupported)?;
            if !plan.remove(annot)? {
                return Err(Unsupported);
            }
            if let Some(field) = annotation_membership(graph, page, annot, true, work)? {
                field_bytes.insert(&mut fields, field).map_err(|_| Budget)?;
                plan.remove(field)?;
            }
        }
        if !fields.is_empty() {
            let (form, _) = form_field_refs(graph, work, Unsupported)?;
            let direct = form == catalog(graph)?;
            plan.keep_array_members(
                graph,
                form,
                "Fields",
                direct,
                |field| !fields.contains(&field),
                work,
            )?;
        }
    }
    let parent = reference(dict(graph, page)?, "Parent")?;
    let parent_values = dict(graph, parent)?;
    let kids = parent_values.get("Kids").ok_or(Malformed)?;
    plan_outline_cleanup(graph, outlines, page, &mut plan, work)?;
    let mut link_bytes = workspace.scope();
    let mut removed_link_edges = BTreeSet::new();
    if let Some(cleanup) = cleanup {
        for (&source, drops) in cleanup {
            let annots = dict(graph, source)?.get("Annots").ok_or(Malformed)?;
            if materialize_links || plan.replacements.contains_key(&source) {
                plan.keep_array_members(
                    graph,
                    source,
                    "Annots",
                    false,
                    |annot| !drops.contains(&annot),
                    work,
                )?;
            } else {
                // @invariant: The link index admits only edges exclusive to this annotation array.
                let container = annots.as_reference().unwrap_or(source);
                spend(work, drops.len())?;
                for &annot in drops {
                    link_bytes
                        .insert(&mut removed_link_edges, (container, annot))
                        .map_err(|_| Budget)?;
                }
            }
            for &annot in drops {
                plan.remove(annot)?;
            }
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
        plan.keep_array_members(graph, parent, "Kids", false, |child| child != page, work)?;
        PageTreeEdges::default()
    };
    page_tree_edges.removed_links = (!removed_link_edges.is_empty()).then_some(&removed_link_edges);
    graph.check_page_removals(
        &plan.removed,
        &plan.replacements,
        page_tree_edges,
        trailer,
        work,
        workspace,
    )?;
    Ok(plan)
}

fn page_tree_reorder(
    graph: &mut NativePdfGraph,
    desired: &[Ref],
) -> Result<(), NativeSessionError> {
    let allowance = graph.remaining_retained();
    let workspace = Workspace::new(allowance);
    let mut map_bytes = workspace.scope();
    let mut changes = BTreeMap::<Ref, BTreeMap<Ref, Ref>>::new();
    for (&before, &after) in graph.pages.iter().zip(desired) {
        if before != after {
            let parent = reference(dict(graph, before)?, "Parent")?;
            let replacements = scoped_entry(&mut changes, parent, &mut map_bytes)?;
            map_bytes.tree_entry::<(Ref, Ref)>().map_err(|_| Budget)?;
            replacements.insert(before, after);
        }
    }
    with_page_scratch(graph, allowance - workspace.remaining(), move |graph| {
        drop(map_bytes);
        for (&parent, replacements) in &changes {
            for &after in replacements.values() {
                if reference(dict(graph, after)?, "Parent")? != parent {
                    let attributes = effective_attributes(graph, after)?;
                    dict_mut(graph, after)?.extend(attributes);
                }
            }
        }
        for (&parent, replacements) in &changes {
            edit_kids(graph, parent, 0, |graph, children| {
                for child in children {
                    if let Some(&replacement) = replacements.get(child) {
                        *child = replacement;
                        dict_mut(graph, replacement)?
                            .insert("Parent".into(), PdfValue::Reference(parent));
                    }
                }
                Ok(())
            })?;
        }
        drop(changes);
        Ok(())
    })
}

pub(super) fn stage(
    graph: &mut NativePdfGraph,
    command: NativePageCommand,
    index: usize,
    projected: &[NativePdfNotePage],
) -> Result<Ref, NativeSessionError> {
    use NativePageCommand::*;
    if !matches!(command, RotatePage { .. })
        && dict(graph, catalog(graph)?)?.contains_key("PageLabels")
    {
        return Err(Unsupported.into());
    }
    let page = graph.pages[index];
    let parent = reference(dict(graph, page)?, "Parent")?;
    if matches!(command, DeletePage { .. }) {
        return stage_delete(graph, index, page, parent);
    }
    let growth = usize::from(matches!(
        command,
        InsertBlankPage { .. } | DuplicatePage { .. }
    ));
    let order_capacity = graph.pages.len().checked_add(growth).ok_or(Budget)?;
    let order_bytes = order_capacity.checked_mul(size_of::<Ref>()).ok_or(Budget)?;
    with_page_scratch(graph, order_bytes, |graph| {
        let mut order = Vec::new();
        order
            .try_reserve_exact(order_capacity)
            .map_err(|_| Budget)?;
        order.extend_from_slice(&graph.pages);
        stage_with_order(graph, command, index, projected, parent, &mut order)
    })
}

fn stage_with_order(
    graph: &mut NativePdfGraph,
    command: NativePageCommand,
    index: usize,
    projected: &[NativePdfNotePage],
    parent: Ref,
    order: &mut Vec<Ref>,
) -> Result<Ref, NativeSessionError> {
    use NativePageCommand::*;
    let page = order[index];
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
            order.remove(index);
            order.insert(position, page);
            page_tree_reorder(graph, order)?;
            page
        }
        InsertBlankPage { .. } | DuplicatePage { .. } => {
            let new = edit_kids(graph, parent, 1, |graph, children| {
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
                        dict_mut(graph, new)?.insert("Annots".into(), PdfValue::Array(annots));
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
                Ok(new)
            })?;
            change_count(graph, parent, 1)?;
            new
        }
        DeletePage { .. } => unreachable!(),
    };
    graph.refresh_page_graph(&BTreeSet::new())?;
    if graph.pages != *order {
        return Err(NativeSessionError::Postcondition);
    }
    Ok(selected)
}

fn stage_delete(
    graph: &mut NativePdfGraph,
    index: usize,
    page: Ref,
    parent: Ref,
) -> Result<Ref, NativeSessionError> {
    if graph.pages.len() == 1 {
        return Err(NativeSessionError::FinalPage);
    }
    let allowance = graph.remaining_retained();
    let workspace = Workspace::new(allowance);
    let order_bytes = workspace
        .reserve(graph.pages.len() * size_of::<Ref>())
        .map_err(|_| Budget)?;
    let mut order = graph.pages.clone();
    order.remove(index);
    let selected = order[index.min(order.len() - 1)];
    let budget = graph.remaining_work();
    let mut work = budget;
    let mut plan = {
        let outlines = outline_index(graph, &mut work, &workspace)?;
        let mut trailer_bytes = workspace.scope();
        let trailer = graph.trailer_references(&mut work, &mut trailer_bytes)?;
        let links = {
            let mut page_bytes = workspace.scope();
            let page_refs = page_reference_set(graph, &mut work, &mut page_bytes)?;
            incoming_link_index(graph, &page_refs, &trailer, &mut work, &workspace)?
        };
        deletion_preflight(
            graph,
            page,
            &outlines,
            &links,
            &trailer,
            &mut PageParentReferences::new(&workspace),
            &mut work,
            true,
        )?
    };
    if !plan.replacements.contains_key(&parent) {
        plan.keep_array_members(
            graph,
            parent,
            "Kids",
            false,
            |child| child != page,
            &mut work,
        )?;
    }
    graph.charge_page_work(budget - work)?;
    let PageDeletion {
        removed,
        replacements,
        removal_bytes,
        entry_bytes,
        value_bytes,
        installed_bytes,
    } = plan;
    graph.charge_page_retained(installed_bytes)?;
    for (owner, values) in replacements {
        *dict_mut(graph, owner)? = values;
    }
    drop((entry_bytes, value_bytes));
    change_count(graph, parent, -1)?;
    let refresh_scratch = allowance - workspace.remaining();
    graph.charge_page_retained(refresh_scratch)?;
    let refresh = graph.refresh_page_graph(&removed);
    let matched_order = graph.pages == order;
    drop((removed, order));
    drop((removal_bytes, order_bytes));
    graph.release_page_retained(refresh_scratch);
    refresh?;
    if !matched_order {
        return Err(NativeSessionError::Postcondition);
    }
    Ok(selected)
}

struct OutlineIndex<'a> {
    root: Option<Ref>,
    siblings: BTreeMap<Ref, (Vec<Ref>, Reservation<'a>)>,
    removable: BTreeMap<Ref, BTreeMap<Ref, BTreeSet<usize>>>,
    blocked: BTreeSet<Ref>,
    bytes: Reservation<'a>,
}

impl OutlineIndex<'_> {
    fn count_changes(
        &self,
        graph: &NativePdfGraph,
        page: Ref,
        work: &mut usize,
        bytes: &mut Reservation<'_>,
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
                *scoped_entry(&mut counts, ancestor, bytes)? =
                    if count < 0 { -magnitude } else { magnitude };
                if ancestor == root || count < 0 {
                    break;
                }
                ancestor = reference(values, "Parent")?;
            }
        }
        Ok(counts)
    }
}

fn outline_index<'a>(
    graph: &NativePdfGraph,
    work: &mut usize,
    workspace: &'a Workspace,
) -> Result<OutlineIndex<'a>, NativeEditError> {
    let mut index = OutlineIndex {
        root: None,
        siblings: BTreeMap::new(),
        removable: BTreeMap::new(),
        blocked: BTreeSet::new(),
        bytes: workspace.scope(),
    };
    let catalog = catalog(graph)?;
    let Some(root) = dict(graph, catalog)?
        .get("Outlines")
        .and_then(PdfValue::as_reference)
    else {
        return Ok(index);
    };
    index.root = Some(root);
    let mut stack = workspace.scope();
    let mut pending = Vec::new();
    stack.push(&mut pending, (root, 0)).map_err(|_| Budget)?;
    let mut seen_bytes = workspace.scope();
    let mut seen = BTreeSet::new();
    seen_bytes.insert(&mut seen, root).map_err(|_| Budget)?;
    while let Some((parent, depth)) = pending.pop() {
        if depth > graph.limits.max_nesting {
            return Err(Unsupported);
        }
        let values = dict(graph, parent)?;
        let last = values.get("Last").and_then(PdfValue::as_reference);
        let mut current = values.get("First").and_then(PdfValue::as_reference);
        let mut previous = None;
        let mut child_bytes = workspace.scope();
        let mut children = Vec::new();
        let mut has_removable = false;
        while let Some(child) = current {
            spend(work, 1)?;
            if !seen_bytes.insert(&mut seen, child).map_err(|_| Budget)? {
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
                let owners = [
                    previous,
                    current,
                    (previous.is_none() || current.is_none()).then_some(parent),
                ];
                let removable = !branch
                    && values
                        .get("Count")
                        .and_then(PdfValue::as_integer)
                        .unwrap_or(0)
                        == 0
                    && graph.reverse.get(&child).is_some_and(|actual| {
                        owners.iter().flatten().all(|owner| actual.contains(owner))
                            && actual.iter().all(|&owner| owners.contains(&Some(owner)))
                    });
                if removable {
                    let groups = scoped_entry(&mut index.removable, page, &mut index.bytes)?;
                    let drops = scoped_entry(groups, parent, &mut index.bytes)?;
                    index
                        .bytes
                        .insert(drops, children.len())
                        .map_err(|_| Budget)?;
                    has_removable = true;
                } else {
                    index
                        .bytes
                        .insert(&mut index.blocked, page)
                        .map_err(|_| Budget)?;
                }
            }
            child_bytes.push(&mut children, child).map_err(|_| Budget)?;
            if branch {
                stack
                    .push(&mut pending, (child, depth + 1))
                    .map_err(|_| Budget)?;
            }
            previous = Some(child);
        }
        if previous != last {
            return Err(Unsupported);
        }
        if has_removable {
            index
                .bytes
                .tree_entry::<(Ref, (Vec<Ref>, Reservation<'_>))>()
                .map_err(|_| Budget)?;
            index.siblings.insert(parent, (children, child_bytes));
        }
    }
    Ok(index)
}

fn plan_outline_cleanup(
    graph: &NativePdfGraph,
    index: &OutlineIndex<'_>,
    page: Ref,
    plan: &mut PageDeletion<'_>,
    work: &mut usize,
) -> Result<(), NativeEditError> {
    let mut count_bytes = index.bytes.workspace().scope();
    let counts = index.count_changes(graph, page, work, &mut count_bytes)?;
    if let Some(groups) = index.removable.get(&page) {
        for (&parent, drops) in groups {
            let (siblings, _) = index.siblings.get(&parent).ok_or(Malformed)?;
            spend(work, drops.len())?;
            let mut positions = drops.iter().copied().peekable();
            while let Some(first) = positions.next() {
                let mut last = first;
                plan.remove(siblings[first])?;
                while positions.next_if_eq(&(last + 1)).is_some() {
                    last += 1;
                    plan.remove(siblings[last])?;
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
        let value = plan
            .edit(graph, ancestor, work)?
            .get_mut("Count")
            .ok_or(Malformed)?;
        *value = PdfValue::Integer(count);
    }
    Ok(())
}

fn set_ref(dict: &mut Dict, key: &str, target: Option<Ref>) {
    if let Some(target) = target {
        if let Some(value) = dict.get_mut(key) {
            *value = PdfValue::Reference(target);
        } else {
            dict.insert(key.into(), PdfValue::Reference(target));
        }
    } else {
        dict.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{NativeEditLimits, NativePdfSession, tests::redaction_fixture};
    use super::*;

    #[test]
    fn planned_order_is_charged_during_graph_refresh() {
        let source = redaction_fixture("");
        let open = || NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
        let original = open();
        let available = original.remaining_retained();
        let workspace = Workspace::new(available);
        let mut rebuild = workspace.scope();
        for owners in original.reverse.values() {
            rebuild.tree_entry::<(Ref, BTreeSet<Ref>)>().unwrap();
            for _ in owners {
                rebuild.tree_entry::<Ref>().unwrap();
            }
        }
        for _ in &original.objects {
            rebuild.tree_entry::<Ref>().unwrap();
        }
        rebuild.grow(4 * size_of::<Ref>()).unwrap();
        let peak = available - workspace.remaining();
        let order_bytes = original.pages.len() * size_of::<Ref>();
        let page = original.pages[0];
        let command = NativePageCommand::ReorderPage {
            target: NativePageTarget {
                page: 0,
                stable_id: u64::from(page.object_number) << 16 | u64::from(page.generation),
            },
            position: 0,
        };

        for short_by in [0, 1] {
            let mut graph = open();
            graph
                .charge_page_retained(available - peak - order_bytes + short_by)
                .unwrap();
            let result = stage(&mut graph, command, 0, &[]);
            if short_by == 0 {
                assert_eq!(result, Ok(page));
                assert_eq!(graph.remaining_retained(), peak + order_bytes);
            } else {
                assert_eq!(result, Err(NativeSessionError::Budget));
            }
        }
    }

    #[test]
    fn widget_copy_debits_the_ownership_walk() {
        let source = redaction_fixture("");
        let open = || {
            let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
            let page = graph.pages[0];
            let widget = allocate(
                &mut graph,
                Dict::from([
                    ("Type".into(), PdfValue::Name("Annot".into())),
                    ("Subtype".into(), PdfValue::Name("Widget".into())),
                    ("P".into(), PdfValue::Reference(page)),
                    ("FT".into(), PdfValue::Name("Tx".into())),
                    ("T".into(), PdfValue::String(b"Customer".to_vec())),
                    (
                        "Rect".into(),
                        PdfValue::Array([10, 10, 20, 20].map(PdfValue::Integer).to_vec()),
                    ),
                ]),
            )
            .unwrap();
            let root = catalog(&graph).unwrap();
            dict_mut(&mut graph, root).unwrap().insert(
                "AcroForm".into(),
                PdfValue::Dictionary(Dict::from([("Fields".into(), refs([widget]))])),
            );
            dict_mut(&mut graph, page)
                .unwrap()
                .insert("Annots".into(), refs([widget]));
            graph.refresh_page_graph(&BTreeSet::new()).unwrap();
            graph.reserve_page_copy(page).unwrap();
            let mut copy = dict(&graph, page).unwrap().clone();
            copy.remove("Annots");
            let target = allocate(&mut graph, copy).unwrap();
            (graph, page, target)
        };
        let (mut graph, page, target) = open();
        let work = graph.remaining_work();
        assert_eq!(copy_annotations(&mut graph, page, target).unwrap().len(), 1);
        let spent = 31;
        assert_eq!(work - graph.remaining_work(), spent);
        for short in [0, 1] {
            let (mut graph, page, target) = open();
            graph.charge_page_work(work - spent + short).unwrap();
            let result = copy_annotations(&mut graph, page, target);
            if short == 0 {
                assert_eq!(result.unwrap().len(), 1);
            } else {
                assert!(matches!(result, Err(NativeSessionError::Budget)));
            }
        }
    }

    #[test]
    fn annotation_copy_reserves_source_scratch_and_indirect_installed_slots() {
        let source = redaction_fixture("");
        for indirect in [false, true] {
            for short_by in [0, 1] {
                let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
                let page = graph.pages[0];
                let note = allocate(
                    &mut graph,
                    Dict::from([
                        ("Type".into(), PdfValue::Name("Annot".into())),
                        ("Subtype".into(), PdfValue::Name("Text".into())),
                        ("P".into(), PdfValue::Reference(page)),
                        (
                            "Rect".into(),
                            PdfValue::Array([10, 10, 20, 20].map(PdfValue::Integer).to_vec()),
                        ),
                        ("Contents".into(), PdfValue::String(b"Note".to_vec())),
                    ]),
                )
                .unwrap();
                let installed = refs([note]);
                let installed = if indirect {
                    PdfValue::Reference(allocate_value(&mut graph, installed).unwrap())
                } else {
                    installed
                };
                dict_mut(&mut graph, page)
                    .unwrap()
                    .insert("Annots".into(), installed.clone());
                graph.refresh_page_graph(&BTreeSet::new()).unwrap();
                graph.reserve_page_copy(page).unwrap();
                let mut copy = dict(&graph, page).unwrap().clone();
                copy.remove("Annots");
                let duplicate = allocate(&mut graph, copy).unwrap();

                let available = graph.remaining_retained();
                let mut remaining = available;
                graph
                    .reserve_page_value(note, &mut graph.remaining_work(), &mut remaining)
                    .unwrap();
                let owner_bytes = available - remaining;
                let scratch_bytes = size_of::<Ref>();
                let installed_bytes = usize::from(indirect) * size_of::<PdfValue>();
                graph
                    .charge_page_retained(
                        available - owner_bytes - scratch_bytes - installed_bytes + short_by,
                    )
                    .unwrap();

                let result = copy_annotations(&mut graph, page, duplicate);
                if short_by == 0 {
                    let values = result.unwrap();
                    assert_eq!(values.len(), 1);
                    let copied = values[0].as_reference().unwrap();
                    assert_ne!(copied, note);
                    assert_eq!(reference(dict(&graph, copied).unwrap(), "P"), Ok(duplicate));
                    assert_eq!(graph.remaining_retained(), scratch_bytes);
                } else {
                    assert!(matches!(result, Err(NativeSessionError::Budget)));
                    assert!(!dict(&graph, duplicate).unwrap().contains_key("Annots"));
                }
                let annots = &dict(&graph, page).unwrap()["Annots"];
                assert_eq!(annots.as_reference(), installed.as_reference());
                let values = graph
                    .resolve_metered(annots, &mut graph.remaining_work())
                    .unwrap();
                assert_eq!(values.as_array().unwrap()[0].as_reference(), Some(note));
            }
        }
    }

    #[test]
    fn kids_replacement_reserves_scratch_and_installed_slots_together() {
        let source = redaction_fixture("");
        for indirect in [false, true] {
            for short_by in [0, 1] {
                let mut graph = NativePdfGraph::open(&source, NativeEditLimits::default()).unwrap();
                let page = graph.pages[0];
                let parent = reference(dict(&graph, page).unwrap(), "Parent").unwrap();
                let copy = dict(&graph, page).unwrap().clone();
                let new = allocate(&mut graph, copy).unwrap();
                let original = indirect.then(|| {
                    let array = allocate_value(&mut graph, refs([page])).unwrap();
                    dict_mut(&mut graph, parent)
                        .unwrap()
                        .insert("Kids".into(), PdfValue::Reference(array));
                    array
                });
                let allowance = graph.remaining_retained();
                let mut remaining = allowance;
                let mut work = graph.remaining_work();
                graph
                    .reserve_page_value(parent, &mut work, &mut remaining)
                    .unwrap();
                let owner_bytes = allowance - remaining;
                let scratch = 2 * size_of::<Ref>();
                let installed = (1 + usize::from(indirect)) * size_of::<PdfValue>();
                graph
                    .charge_page_retained(allowance - owner_bytes - scratch - installed + short_by)
                    .unwrap();

                let result = edit_kids(&mut graph, parent, 1, |_, children| {
                    children.push(new);
                    Ok(())
                });
                if short_by == 1 {
                    assert_eq!(result, Err(NativeSessionError::Budget));
                    let slot = &dict(&graph, parent).unwrap()["Kids"];
                    if let Some(array) = original {
                        assert_eq!(slot.as_reference(), Some(array));
                    } else {
                        assert_eq!(slot.as_array().unwrap().len(), 1);
                    }
                } else {
                    result.unwrap();
                    assert_eq!(graph.remaining_retained(), scratch);
                    let kids = dict(&graph, parent).unwrap()["Kids"].as_array().unwrap();
                    assert!(
                        kids.iter()
                            .map(PdfValue::as_reference)
                            .eq([Some(page), Some(new)])
                    );
                }
                if let Some(array) = original {
                    assert_eq!(graph.objects[&array].as_array().unwrap().len(), 1);
                }
            }
        }
    }

    #[test]
    fn indirect_print_preferences_share_the_page_projection_work_budget() {
        let limits = NativeEditLimits::default();
        let source = redaction_fixture("");
        let mut graph = NativePdfGraph::open(&source, limits).unwrap();
        let range = allocate_value(&mut graph, PdfValue::Null).unwrap();
        let preferences = allocate(
            &mut graph,
            BTreeMap::from([("PrintPageRange".into(), PdfValue::Reference(range))]),
        )
        .unwrap();
        let root = catalog(&graph).unwrap();
        dict_mut(&mut graph, root)
            .unwrap()
            .insert("ViewerPreferences".into(), PdfValue::Reference(preferences));
        graph.refresh_page_graph(&BTreeSet::new()).unwrap();
        let bytes = graph.rewrite().unwrap();
        let session = NativePdfSession::open_scoped(&bytes, limits, [3; 16]).unwrap();
        let projected = &session.snapshot().scene.pages;
        let check = |work| {
            let mut graph = NativePdfGraph::open(&bytes, limits).unwrap();
            graph
                .charge_page_work(graph.remaining_work() - work)
                .unwrap();
            let workspace = Workspace::new(graph.remaining_retained());
            restrictions(&graph, projected, &workspace)
        };
        for mut work in [0, 1] {
            assert_eq!(positions_locked(&graph, &mut work), Err(Budget));
        }
        assert!(matches!(check(11), Err(Budget)));
        assert!(check(12).unwrap().is_none());
    }
}
