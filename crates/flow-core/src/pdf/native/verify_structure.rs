use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use miniz_oxide::inflate::{
    TINFLStatus,
    core::{DecompressorOxide, decompress, inflate_flags},
};

use super::super::reader::{
    PdfObjectRef, PdfReadError, PdfReaderLimits, PdfStream, PdfValue, open_pdf_retained,
    single_filter,
};
use super::verifier::{
    MAX_COORD, MAX_PAGES, NativeRedactionGeometry, NativeRedactionMark,
    NativeRedactionPageEvidence,
    NativeVerificationFailure::{Budget, Coverage, Structure, UnsupportedProfile},
    POINT, Rect, Reservation, VerifyResult, Workspace, affected_pages, contains, spend, valid_rect,
};

pub(super) const MAX_INPUT: usize = 32 * 1024 * 1024;
const MAX_RETAINED: usize = 80 * 1024 * 1024;
const MAX_DECODED: usize = 32 * 1024 * 1024;
const MAX_WORK: usize = 350_000;
const MAX_OBJECTS: usize = 4096;
const MAX_DEPTH: usize = 32;

pub(super) type Dict = BTreeMap<String, PdfValue>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Accepted,
    Candidate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Catalog,
    Tree,
    Page,
    Content,
    PageContents,
    Resources,
    FontResources,
    XResources,
    Font,
    CidFont,
    FontDescriptor,
    FontFile,
    UnicodeMap,
    GlyphMap,
    XObject,
    Annotation,
    Length,
    Info,
    Metadata,
    SourceEnvelope,
    Box,
    Widths,
    AnnotationList,
}

pub(super) struct Page<'a> {
    pub geometry: NativeRedactionGeometry,
    pub contents: Vec<PdfObjectRef>,
    pub resources: Option<&'a Dict>,
    pub annotations: Vec<(PdfObjectRef, Rect, &'a [u8])>,
}

pub(super) struct Document {
    objects: BTreeMap<PdfObjectRef, (Role, PdfValue)>,
    pages: Vec<PdfObjectRef>,
    pub side: Side,
}

pub(super) fn verify(
    accepted: &[u8],
    candidate: &[u8],
    order: &[u64],
    marks: &[NativeRedactionMark],
    evidence_bytes: &mut Reservation<'_>,
) -> VerifyResult<Vec<NativeRedactionPageEvidence>> {
    let workspace = evidence_bytes.workspace();
    let mut documents = workspace.scope();
    let accepted = Document::open(accepted, Side::Accepted, workspace, &mut documents)?;
    let candidate = Document::open(candidate, Side::Candidate, workspace, &mut documents)?;
    coverage(&accepted, &candidate, order, marks, evidence_bytes)
}

fn read_error(error: PdfReadError) -> super::verifier::NativeVerificationFailure {
    use PdfReadError::*;
    match error {
        InputSizeLimit | ObjectLimit | PageCountLimit | NestingLimit | StreamLimit
        | DecodedStreamLimit | TokenLimit | ImageLimit => Budget,
        FilterUnsupported | EncryptionUnsupported | XrefUnsupported => UnsupportedProfile,
        _ => Structure,
    }
}

impl Document {
    pub fn open(
        bytes: &[u8],
        side: Side,
        workspace: &Workspace,
        documents: &mut Reservation<'_>,
    ) -> VerifyResult<Self> {
        let _reader_overhead = workspace.reserve(128)?;
        let max_retained = workspace.remaining().min(MAX_RETAINED);
        let reader = open_pdf_retained(
            bytes,
            PdfReaderLimits {
                max_input_bytes: MAX_INPUT,
                max_objects: MAX_OBJECTS,
                max_pages: MAX_PAGES,
                max_nesting: MAX_DEPTH,
                max_image_pixels: 16_000_000,
                ..PdfReaderLimits::default()
            },
            max_retained,
        )
        .map_err(read_error)?;
        let reader_cost = bytes.len() + reader.retained_cost;
        let _reader_bytes = workspace.reserve(reader_cost)?;
        let trailer = reader.retained_trailer();
        let trailer_keys = match side {
            Side::Accepted => &["Root", "Size", "Info", "ID"][..],
            Side::Candidate => &["Root", "Size"][..],
        };
        keys(trailer, trailer_keys)?;
        if let Some(id) = trailer.get("ID") {
            let [PdfValue::String(first), PdfValue::String(second)] =
                id.as_array().ok_or(Structure)?
            else {
                return Err(Structure);
            };
            if first.len().max(second.len()) > 128 {
                return Err(UnsupportedProfile);
            }
        }
        let root = reference(get(trailer, "Root")?)?;
        let mut queue_bytes = workspace.reserve(4 * size_of::<(PdfObjectRef, Role)>())?;
        let mut queue = Vec::with_capacity(4);
        queue.push((root, Role::Catalog));
        if let Some(info) = trailer.get("Info") {
            queue.push((reference(info)?, Role::Info));
        }
        let mut document = Self {
            objects: BTreeMap::new(),
            pages: Vec::new(),
            side,
        };
        let mut work = MAX_WORK.checked_sub(reader.open_work).ok_or(Budget)?;
        let mut retained = max_retained.checked_sub(reader_cost).ok_or(Budget)?;
        let mut decoded = MAX_DECODED;
        while let Some((id, role)) = queue.pop() {
            spend(&mut work, 1)?;
            if let Some((seen, previous)) = document.objects.get(&id) {
                if *seen != role
                    && !matches!(
                        (*seen, role),
                        (Role::Page, Role::Tree) | (Role::Tree, Role::Page)
                    )
                    && !matches!(
                        (*seen, role, previous),
                        (Role::Content, Role::PageContents, PdfValue::Stream(_))
                    )
                {
                    return Err(Structure);
                }
                continue;
            }
            documents.tree_entry::<(PdfObjectRef, Role, PdfValue)>()?;
            let (value, cost, tokens) = reader
                .retained_object(id, retained.min(workspace.remaining()), decoded, work)
                .map_err(read_error)?;
            documents.grow(cost)?;
            spend(&mut retained, cost)?;
            spend(&mut work, tokens)?;
            if let PdfValue::Stream(stream) = &value {
                spend(&mut decoded, stream.data.len())?;
            }
            let role = match (role, value.as_dictionary().and_then(|d| name(d, "Type"))) {
                (Role::Tree, Some("Page")) => Role::Page,
                (Role::PageContents, _) if matches!(value, PdfValue::Stream(_)) => Role::Content,
                _ => role,
            };
            // @invariant: Every newly enqueued edge uses a reference token in this object.
            queue_bytes.grow(2 * tokens * size_of::<(PdfObjectRef, Role)>())?;
            inspect(&value, role, side, &mut queue)?;
            if let PdfValue::Stream(stream) = &value
                && side == Side::Candidate
                && stream.dict.contains_key("Filter")
            {
                exact_flate(stream, workspace)?;
            }
            document.objects.insert(id, (role, value));
        }
        drop(queue);
        drop(queue_bytes);
        let tree = reference(get(document.dict(root)?, "Pages")?)?;
        let mut pages = Vec::new();
        {
            let mut tree_bytes = workspace.scope();
            let mut visited = BTreeSet::new();
            document.collect_pages(tree, None, &mut visited, &mut pages, 0, &mut tree_bytes)?;
        }
        documents.grow(pages.capacity() * size_of::<PdfObjectRef>())?;
        if pages.is_empty() {
            return Err(Structure);
        }
        document.pages = pages;
        if side == Side::Candidate {
            let size = usize::try_from(get(trailer, "Size")?.as_integer().ok_or(Structure)?)
                .map_err(|_| Structure)?;
            document.check_physical(bytes, &reader, size, workspace)?;
        }
        Ok(document)
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_pages(
        &self,
        id: PdfObjectRef,
        parent: Option<PdfObjectRef>,
        visited: &mut BTreeSet<PdfObjectRef>,
        pages: &mut Vec<PdfObjectRef>,
        depth: usize,
        tree_bytes: &mut Reservation<'_>,
    ) -> VerifyResult<usize> {
        if depth >= MAX_DEPTH || pages.len() >= MAX_PAGES {
            return Err(Budget);
        }
        if !tree_bytes.insert(visited, id)? {
            return Err(Structure);
        }
        let dict = self.dict(id)?;
        if dict.get("Parent").map(reference).transpose()? != parent {
            return Err(Structure);
        }
        match name(dict, "Type") {
            Some("Page") if parent.is_some() => {
                tree_bytes.push(pages, id)?;
                Ok(1)
            }
            Some("Pages") => {
                let kids = get(dict, "Kids")?.as_array().ok_or(Structure)?;
                if kids.is_empty() {
                    return Err(Structure);
                }
                let mut count = 0;
                for kid in kids {
                    count += self.collect_pages(
                        reference(kid)?,
                        Some(id),
                        visited,
                        pages,
                        depth + 1,
                        tree_bytes,
                    )?;
                }
                if get(dict, "Count")?.as_integer() != Some(count as i64) {
                    return Err(Structure);
                }
                Ok(count)
            }
            _ => Err(Structure),
        }
    }

    pub fn object(&self, id: PdfObjectRef) -> VerifyResult<&PdfValue> {
        self.objects
            .get(&id)
            .map(|(_, value)| value)
            .ok_or(Structure)
    }

    pub fn resolve<'a>(&'a self, value: &'a PdfValue) -> VerifyResult<&'a PdfValue> {
        match value {
            PdfValue::Reference(id) => self.object(*id),
            value => Ok(value),
        }
    }

    pub fn dict(&self, id: PdfObjectRef) -> VerifyResult<&Dict> {
        self.object(id)?.as_dictionary().ok_or(Structure)
    }

    pub fn stream(&self, id: PdfObjectRef) -> VerifyResult<&PdfStream> {
        match self.object(id)? {
            PdfValue::Stream(stream) => Ok(stream),
            _ => Err(Structure),
        }
    }

    pub fn dictionary<'a>(&'a self, value: &'a PdfValue) -> VerifyResult<&'a Dict> {
        self.resolve(value)?.as_dictionary().ok_or(Structure)
    }

    pub fn assets(&self) -> impl Iterator<Item = PdfObjectRef> + '_ {
        self.objects
            .iter()
            .filter_map(|(id, (role, _))| matches!(role, Role::Font | Role::XObject).then_some(*id))
    }

    pub fn page(&self, index: usize, page_bytes: &mut Reservation<'_>) -> VerifyResult<Page<'_>> {
        let id = *self.pages.get(index).ok_or(Coverage)?;
        let dict = self.dict(id)?;
        let mut media = None;
        let mut crop = None;
        let mut rotation = None;
        let mut resources = None;
        let mut ancestor = Some(id);
        while let Some(parent) = ancestor {
            let node = self.dict(parent)?;
            if media.is_none() {
                media = node
                    .get("MediaBox")
                    .map(|value| rect(self.resolve(value)?))
                    .transpose()?;
            }
            if crop.is_none() {
                crop = node
                    .get("CropBox")
                    .map(|value| rect(self.resolve(value)?))
                    .transpose()?;
            }
            if rotation.is_none() {
                rotation = node
                    .get("Rotate")
                    .map(|value| {
                        let angle = value.as_integer().ok_or(Structure)?.rem_euclid(360) as u16;
                        angle
                            .is_multiple_of(90)
                            .then_some(angle)
                            .ok_or(UnsupportedProfile)
                    })
                    .transpose()?;
            }
            if resources.is_none() {
                resources = node
                    .get("Resources")
                    .map(|value| self.dictionary(value))
                    .transpose()?;
            }
            ancestor = node.get("Parent").map(reference).transpose()?;
        }
        let media_box = media.ok_or(Structure)?;
        let crop_box = crop.unwrap_or(media_box);
        if !contains(media_box, crop_box) {
            return Err(UnsupportedProfile);
        }
        let geometry = NativeRedactionGeometry {
            media_box,
            crop_box,
            rotation: rotation.unwrap_or(0),
        };
        let contents = match dict
            .get("Contents")
            .map(|value| self.resolve(value))
            .transpose()?
        {
            Some(PdfValue::Stream(_)) => {
                page_bytes.grow(size_of::<PdfObjectRef>())?;
                vec![reference(get(dict, "Contents")?)?]
            }
            Some(PdfValue::Array(values)) => {
                page_bytes.grow(values.len() * size_of::<PdfObjectRef>())?;
                let mut contents = Vec::with_capacity(values.len());
                for value in values {
                    contents.push(reference(value)?);
                }
                contents
            }
            None | Some(PdfValue::Null) => Vec::new(),
            _ => return Err(Structure),
        };
        for &content in &contents {
            self.stream(content)?;
        }
        let mut annotations = Vec::new();
        if let Some(annots) = dict.get("Annots") {
            for item in self.resolve(annots)?.as_array().ok_or(Structure)? {
                let note = self.dict(reference(item)?)?;
                if note
                    .get("P")
                    .map(reference)
                    .transpose()?
                    .is_some_and(|page| page != id)
                {
                    return Err(Structure);
                }
                let bounds = rect(get(note, "Rect")?)?;
                if !contains(crop_box, bounds) {
                    return Err(UnsupportedProfile);
                }
                page_bytes.push(
                    &mut annotations,
                    (
                        reference(item)?,
                        bounds,
                        get(note, "Contents")?.as_bytes().ok_or(Structure)?,
                    ),
                )?;
            }
        }
        Ok(Page {
            geometry,
            contents,
            resources,
            annotations,
        })
    }

    fn check_physical(
        &self,
        bytes: &[u8],
        reader: &super::super::reader::PdfReader,
        size: usize,
        workspace: &Workspace,
    ) -> VerifyResult<()> {
        if size != self.objects.len() + 1 {
            return Err(Structure);
        }
        let header_end = bytes
            .iter()
            .position(|byte| matches!(byte, b'\n' | b'\r'))
            .ok_or(Structure)?;
        if header_end != 8 {
            return Err(Structure);
        }
        let mut rest = trim_pdf(&bytes[header_end..]);
        if let Some(after) = rest.strip_prefix(b"%\xE2\xE3\xCF\xD3") {
            if !matches!(after.first(), Some(b'\n' | b'\r')) {
                return Err(Structure);
            }
            rest = trim_pdf(after);
        }
        let _offset_bytes = workspace.reserve((size - 1) * size_of::<usize>())?;
        let mut offsets = Vec::with_capacity(size - 1);
        for (index, (&id, (_, value))) in self.objects.iter().enumerate() {
            let offset = bytes.len() - rest.len();
            if id.object_number as usize != index + 1
                || id.generation != 0
                || reader.object_offset(id) != Some(offset)
            {
                return Err(Structure);
            }
            offsets.push(offset);
            rest = checked_object(rest, value)?;
        }
        let offset = bytes.len() - rest.len();
        for token in [b"xref".as_slice(), b"0"] {
            expect_token(&mut rest, token)?;
        }
        expect_token(&mut rest, size.to_string().as_bytes())?;
        expect_token(&mut rest, b"0000000000")?;
        expect_token(&mut rest, b"65535")?;
        expect_token(&mut rest, b"f")?;
        for start in offsets {
            expect_token(&mut rest, format!("{start:010}").as_bytes())?;
            expect_token(&mut rest, b"00000")?;
            expect_token(&mut rest, b"n")?;
        }
        expect_token(&mut rest, b"trailer")?;
        loop {
            if token(&mut rest)? == b"startxref" {
                break;
            }
        }
        expect_token(&mut rest, offset.to_string().as_bytes())?;
        if trim_pdf(rest).strip_prefix(b"%%EOF").map(trim_pdf) != Some(b"") {
            return Err(Structure);
        }
        Ok(())
    }
}

pub(super) fn coverage(
    accepted: &Document,
    candidate: &Document,
    order: &[u64],
    marks: &[NativeRedactionMark],
    evidence_bytes: &mut Reservation<'_>,
) -> VerifyResult<Vec<NativeRedactionPageEvidence>> {
    if order.len() != accepted.pages.len() || order.len() != candidate.pages.len() {
        return Err(Coverage);
    }
    let workspace = evidence_bytes.workspace();
    let mut covered = evidence_bytes.page_evidence(affected_pages(order, marks).count())?;
    for (index, &page_id) in order.iter().enumerate() {
        let geometry = {
            let mut page_bytes = workspace.scope();
            accepted.page(index, &mut page_bytes)?.geometry
        };
        let candidate_geometry = {
            let mut page_bytes = workspace.scope();
            candidate.page(index, &mut page_bytes)?.geometry
        };
        if candidate_geometry != geometry {
            return Err(Coverage);
        }
        let mut marked = false;
        for mark in marks.iter().filter(|mark| mark.page_id == page_id) {
            if mark.geometry != geometry {
                return Err(Coverage);
            }
            marked = true;
        }
        if marked {
            covered.push(NativeRedactionPageEvidence { page_id, geometry });
        }
    }
    Ok(covered)
}

fn inspect(
    value: &PdfValue,
    role: Role,
    side: Side,
    queue: &mut Vec<(PdfObjectRef, Role)>,
) -> VerifyResult<()> {
    use Role::*;
    let mut enqueue = |value: &PdfValue, role| {
        queue.push((reference(value)?, role));
        Ok(())
    };
    match role {
        Length => {
            return value
                .as_integer()
                .filter(|n| *n >= 0)
                .map(|_| ())
                .ok_or(Structure);
        }
        Box => return rect(value).map(|_| ()),
        Widths => return only_numbers(value, 0),
        AnnotationList => {
            return value
                .as_array()
                .ok_or(Structure)?
                .iter()
                .try_for_each(|v| enqueue(v, Annotation));
        }
        PageContents if matches!(value, PdfValue::Null) => return Ok(()),
        PageContents if value.as_array().is_some() => {
            return value
                .as_array()
                .ok_or(Structure)?
                .iter()
                .try_for_each(|v| enqueue(v, Content));
        }
        _ => {}
    }
    if let PdfValue::Stream(stream) = value {
        let mut allowed = vec!["Length", "Filter"];
        match role {
            FontFile => allowed.push("Length1"),
            XObject => match name(&stream.dict, "Subtype") {
                Some("Image") => allowed.extend([
                    "Type",
                    "Subtype",
                    "Width",
                    "Height",
                    "BitsPerComponent",
                    "ColorSpace",
                    "Interpolate",
                ]),
                Some("Form") => {
                    allowed.extend(["Type", "Subtype", "BBox", "Matrix", "Resources", "FormType"])
                }
                _ => return Err(UnsupportedProfile),
            },
            Metadata | SourceEnvelope if side == Side::Accepted => {
                allowed.extend(["Type", "Subtype"])
            }
            Content | UnicodeMap | GlyphMap => {}
            _ => return Err(Structure),
        }
        keys(&stream.dict, &allowed)?;
        let length = get(&stream.dict, "Length")?;
        if matches!(length, PdfValue::Reference(_)) {
            enqueue(length, Length)?;
        } else if length.as_integer() != i64::try_from(stream.encoded.len()).ok() {
            return Err(Structure);
        }
        match stream.dict.get("Filter") {
            None => {}
            Some(filter) if matches!(filter.as_name(), Some("FlateDecode" | "Fl")) => {}
            Some(filter)
                if side == Side::Accepted
                    && role == XObject
                    && name(&stream.dict, "Subtype") == Some("Image")
                    && matches!(single_filter(filter), Some("DCTDecode" | "DCT")) => {}
            _ => return Err(UnsupportedProfile),
        }
        if role == XObject {
            if stream.dict.contains_key("Type") && name(&stream.dict, "Type") != Some("XObject") {
                return Err(Structure);
            }
            if let Some(resources) = stream.dict.get("Resources") {
                inspect_entry(resources, Resources, side, queue)?;
            }
        } else if matches!(role, Metadata | SourceEnvelope)
            && (name(&stream.dict, "Type") != Some("Metadata")
                || name(&stream.dict, "Subtype")
                    != Some(if role == Metadata {
                        "XML"
                    } else {
                        "FlowPDF.Source"
                    }))
        {
            return Err(UnsupportedProfile);
        } else if role == FontFile {
            if get(&stream.dict, "Length1")?.as_integer() != i64::try_from(stream.data.len()).ok() {
                return Err(Structure);
            }
            inspect_font_program(&stream.data)?;
        }
        return Ok(());
    }
    let dict = value.as_dictionary().ok_or(Structure)?;
    let (allowed, required_type): (&[&str], _) = match role {
        Catalog => (
            &["Type", "Pages", "Metadata", "FlowPDFSource"],
            Some("Catalog"),
        ),
        Tree => (
            &[
                "Type",
                "Kids",
                "Count",
                "Parent",
                "Resources",
                "MediaBox",
                "CropBox",
                "Rotate",
            ],
            Some("Pages"),
        ),
        Page => (
            &[
                "Type",
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
            Some("Page"),
        ),
        Resources => (&["Font", "XObject", "ProcSet"], None),
        FontResources | XResources => {
            for (key, value) in dict {
                if key.is_empty() || !key.bytes().all(|c| c.is_ascii_graphic() && c != b'#') {
                    return Err(UnsupportedProfile);
                }
                enqueue(value, if role == FontResources { Font } else { XObject })?;
            }
            return Ok(());
        }
        Font => match name(dict, "Subtype") {
            Some("TrueType") => (
                &[
                    "Type",
                    "Subtype",
                    "BaseFont",
                    "Encoding",
                    "FirstChar",
                    "LastChar",
                    "Widths",
                    "FontDescriptor",
                    "ToUnicode",
                ],
                Some("Font"),
            ),
            Some("Type0") => (
                &[
                    "Type",
                    "Subtype",
                    "BaseFont",
                    "Encoding",
                    "DescendantFonts",
                    "ToUnicode",
                ],
                Some("Font"),
            ),
            _ => return Err(UnsupportedProfile),
        },
        CidFont => (
            &[
                "Type",
                "Subtype",
                "BaseFont",
                "CIDSystemInfo",
                "CIDToGIDMap",
                "FontDescriptor",
                "DW",
                "W",
            ],
            Some("Font"),
        ),
        FontDescriptor => (
            &[
                "Type",
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
            Some("FontDescriptor"),
        ),
        Annotation => (&["Type", "Subtype", "Rect", "Contents", "P"], None),
        Info if side == Side::Accepted => (
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
            None,
        ),
        _ => return Err(Structure),
    };
    keys(dict, allowed)?;
    if required_type.is_some() && name(dict, "Type") != required_type {
        return Err(Structure);
    }
    match role {
        Catalog => enqueue(get(dict, "Pages")?, Tree)?,
        Tree => {
            for kid in get(dict, "Kids")?.as_array().ok_or(Structure)? {
                enqueue(kid, Tree)?;
            }
        }
        Resources => {
            if let Some(value) = dict.get("ProcSet") {
                for item in value.as_array().ok_or(Structure)? {
                    if !matches!(
                        item.as_name(),
                        Some("PDF" | "Text" | "ImageB" | "ImageC" | "ImageI")
                    ) {
                        return Err(UnsupportedProfile);
                    }
                }
            }
        }
        Page => {
            if dict
                .get("UserUnit")
                .is_some_and(|v| v.as_integer() != Some(1))
            {
                return Err(UnsupportedProfile);
            }
            if let Some(value) = dict.get("Contents") {
                match value {
                    PdfValue::Null => {}
                    PdfValue::Reference(_) => enqueue(value, PageContents)?,
                    PdfValue::Array(values) => {
                        for item in values {
                            enqueue(item, Content)?;
                        }
                    }
                    _ => return Err(Structure),
                }
            }
            if let Some(value) = dict.get("Annots") {
                inspect_entry(value, AnnotationList, side, queue)?;
            }
        }
        Annotation => {
            if name(dict, "Subtype") != Some("Text")
                || dict
                    .get("Type")
                    .is_some_and(|v| v.as_name() != Some("Annot"))
                || get(dict, "Contents")?
                    .as_bytes()
                    .filter(|v| v.len() <= 32_770)
                    .is_none()
            {
                return Err(UnsupportedProfile);
            }
            rect(get(dict, "Rect")?)?;
        }
        Font | CidFont | FontDescriptor => {
            for (key, value) in dict {
                match key.as_str() {
                    "DescendantFonts" => {
                        let [child] = value.as_array().ok_or(Structure)? else {
                            return Err(Structure);
                        };
                        enqueue(child, CidFont)?;
                    }
                    "CIDSystemInfo" => {
                        let info = value.as_dictionary().ok_or(Structure)?;
                        keys(info, &["Registry", "Ordering", "Supplement"])?;
                        if get(info, "Registry")?.as_bytes() != Some(b"Adobe")
                            || get(info, "Ordering")?.as_bytes() != Some(b"Identity")
                            || !get(info, "Supplement")?
                                .as_integer()
                                .is_some_and(|n| (0..=10).contains(&n))
                        {
                            return Err(UnsupportedProfile);
                        }
                    }
                    "CIDToGIDMap" if value.as_name() == Some("Identity") => {}
                    "CIDToGIDMap" => enqueue(value, GlyphMap)?,
                    "ToUnicode" => enqueue(value, UnicodeMap)?,
                    "FontDescriptor" => enqueue(value, FontDescriptor)?,
                    "FontFile2" => enqueue(value, FontFile)?,
                    "Widths" | "W" if matches!(value, PdfValue::Reference(_)) => {
                        enqueue(value, Widths)?
                    }
                    "Widths" | "W" => only_numbers(value, 0)?,
                    "FontBBox" => {
                        let [l, b, r, t] = value.as_array().ok_or(Structure)? else {
                            return Err(Structure);
                        };
                        for boundary in [l, b, r, t] {
                            number(boundary)?;
                        }
                    }
                    "BaseFont" | "FontName" => {
                        let name = value.as_name().ok_or(Structure)?;
                        if !(1..=63).contains(&name.len())
                            || name.bytes().any(|b| !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'+' | b'-'))
                        {
                            return Err(UnsupportedProfile);
                        }
                    }
                    "Encoding" => match (name(dict, "Subtype"), value.as_name()) {
                        (Some("Type0"), Some("Identity-H"))
                        | (Some("TrueType"), Some("WinAnsiEncoding")) => {}
                        _ => return Err(UnsupportedProfile),
                    },
                    "Type" | "Subtype" => {
                        value.as_name().ok_or(Structure)?;
                    }
                    _ => {
                        number(value)?;
                    }
                }
            }
        }
        Info => {
            for (key, value) in dict {
                if key == "Trapped" {
                    if !matches!(value.as_name(), Some("True" | "False" | "Unknown")) {
                        return Err(UnsupportedProfile);
                    }
                } else {
                    value.as_bytes().ok_or(Structure)?;
                }
            }
        }
        _ => {}
    }
    for (key, role) in [
        ("Resources", Resources),
        ("Font", FontResources),
        ("XObject", XResources),
        ("MediaBox", Box),
        ("CropBox", Box),
        ("Metadata", Metadata),
        ("FlowPDFSource", SourceEnvelope),
    ] {
        if let Some(value) = dict.get(key) {
            if matches!(role, Metadata | SourceEnvelope) {
                if side == Side::Candidate {
                    return Err(Structure);
                }
                queue.push((reference(value)?, role));
            } else {
                inspect_entry(value, role, side, queue)?;
            }
        }
    }
    Ok(())
}

fn inspect_font_program(bytes: &[u8]) -> VerifyResult<()> {
    if bytes != include_bytes!("../../../data/NotoSans-Regular.ttf") {
        return Err(UnsupportedProfile);
    }
    Ok(())
}

fn inspect_entry(
    value: &PdfValue,
    role: Role,
    side: Side,
    queue: &mut Vec<(PdfObjectRef, Role)>,
) -> VerifyResult<()> {
    if let PdfValue::Reference(id) = value {
        queue.push((*id, role));
        Ok(())
    } else {
        inspect(value, role, side, queue)
    }
}

pub(super) fn keys(dict: &Dict, allowed: &[&str]) -> VerifyResult<()> {
    if dict.keys().all(|key| allowed.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(Structure)
    }
}

pub(super) fn get<'a>(dict: &'a Dict, key: &str) -> VerifyResult<&'a PdfValue> {
    dict.get(key).ok_or(Structure)
}

pub(super) fn name<'a>(dict: &'a Dict, key: &str) -> Option<&'a str> {
    dict.get(key).and_then(PdfValue::as_name)
}

pub(super) fn reference(value: &PdfValue) -> VerifyResult<PdfObjectRef> {
    value.as_reference().ok_or(Structure)
}

fn only_numbers(value: &PdfValue, depth: usize) -> VerifyResult<()> {
    if depth >= 2 {
        return number(value).map(|_| ());
    }
    let list = value.as_array().ok_or(Structure)?;
    for item in list {
        if item.as_array().is_some() {
            only_numbers(item, depth + 1)?;
        } else {
            number(item)?;
        }
    }
    Ok(())
}

pub(super) fn number(value: &PdfValue) -> VerifyResult<i64> {
    match value {
        PdfValue::Integer(n) => n
            .checked_mul(POINT)
            .filter(|n| (-MAX_COORD..=MAX_COORD).contains(n))
            .ok_or(UnsupportedProfile),
        PdfValue::ExactReal(token) => decimal(token.as_bytes()),
        _ => Err(Structure),
    }
}

pub(super) fn decimal(mut token: &[u8]) -> VerifyResult<i64> {
    let negative = token.first() == Some(&b'-');
    if matches!(token.first(), Some(b'+' | b'-')) {
        token = &token[1..];
    }
    let mut whole = 0_i64;
    let mut fraction = 0;
    let mut place = POINT;
    let mut fractional = false;
    let mut digits = 0;
    for &byte in token {
        if byte == b'.' && !fractional {
            fractional = true;
        } else if byte.is_ascii_digit() {
            digits += 1;
            let digit = i64::from(byte - b'0');
            if fractional {
                place /= 10;
                if place == 0 && digit != 0 {
                    return Err(UnsupportedProfile);
                }
                fraction += place * digit;
            } else {
                whole = whole * 10 + digit;
                if whole > MAX_COORD / POINT {
                    return Err(UnsupportedProfile);
                }
            }
        } else {
            return Err(Structure);
        }
    }
    let result = whole * POINT + fraction;
    if digits == 0 || result > MAX_COORD {
        return Err(UnsupportedProfile);
    }
    Ok(if negative { -result } else { result })
}

pub(super) fn rect(value: &PdfValue) -> VerifyResult<Rect> {
    let [l, b, r, t] = value.as_array().ok_or(Structure)? else {
        return Err(Structure);
    };
    let bounds = [number(l)?, number(b)?, number(r)?, number(t)?];
    valid_rect(bounds)
        .then_some(bounds)
        .ok_or(UnsupportedProfile)
}

fn exact_flate(stream: &PdfStream, workspace: &Workspace) -> VerifyResult<()> {
    let _scratch = workspace.reserve(stream.data.len() + 1 + size_of::<DecompressorOxide>())?;
    let mut check = vec![0; stream.data.len() + 1];
    let (status, consumed, written) = decompress(
        &mut DecompressorOxide::new(),
        &stream.encoded,
        &mut check,
        0,
        inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
            | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    );
    if status != TINFLStatus::Done
        || consumed != stream.encoded.len()
        || written != stream.data.len()
        || check[..written] != stream.data
    {
        return Err(Structure);
    }
    Ok(())
}

pub(super) fn pdf_space(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 12 | b'\r' | b' ')
}

fn trim_pdf(bytes: &[u8]) -> &[u8] {
    &bytes[bytes
        .iter()
        .position(|byte| !pdf_space(*byte))
        .unwrap_or(bytes.len())..]
}

fn delimiter(byte: u8) -> bool {
    pdf_space(byte) || b"()<>[]{}/%".contains(&byte)
}

pub(super) fn token<'a>(rest: &mut &'a [u8]) -> VerifyResult<&'a [u8]> {
    let bytes = trim_pdf(rest);
    let Some(&first) = bytes.first() else {
        return Err(Structure);
    };
    let end = match first {
        b'%' => return Err(Structure),
        b'(' => {
            let mut depth = 1;
            let mut escaped = false;
            let mut end = None;
            for (index, &byte) in bytes.iter().enumerate().skip(1) {
                if escaped {
                    escaped = false;
                    continue;
                }
                match byte {
                    b'\\' => escaped = true,
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(index + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            end.ok_or(Structure)?
        }
        b'<' if bytes.get(1) != Some(&b'<') => {
            bytes.iter().position(|b| *b == b'>').ok_or(Structure)? + 1
        }
        b'<' | b'>' if bytes.get(1) == Some(&first) => 2,
        b'/' => {
            1 + bytes[1..]
                .iter()
                .position(|b| delimiter(*b))
                .unwrap_or(bytes.len() - 1)
        }
        byte if delimiter(byte) => 1,
        _ => bytes
            .iter()
            .position(|b| delimiter(*b))
            .unwrap_or(bytes.len()),
    };
    *rest = &bytes[end..];
    Ok(&bytes[..end])
}

fn expect_token(rest: &mut &[u8], expected: &[u8]) -> VerifyResult<()> {
    (token(rest)? == expected).then_some(()).ok_or(Structure)
}

fn checked_object<'a>(mut rest: &'a [u8], value: &PdfValue) -> VerifyResult<&'a [u8]> {
    let _ = token(&mut rest)?;
    let _ = token(&mut rest)?;
    expect_token(&mut rest, b"obj")?;
    loop {
        match token(&mut rest)? {
            b"endobj" if !matches!(value, PdfValue::Stream(_)) => return Ok(trim_pdf(rest)),
            b"stream" => {
                let PdfValue::Stream(stream) = value else {
                    return Err(Structure);
                };
                rest = rest
                    .strip_prefix(b"\r\n")
                    .or_else(|| rest.strip_prefix(b"\n"))
                    .or_else(|| rest.strip_prefix(b"\r"))
                    .ok_or(Structure)?;
                rest = rest
                    .strip_prefix(stream.encoded.as_slice())
                    .ok_or(Structure)?;
                expect_token(&mut rest, b"endstream")?;
                expect_token(&mut rest, b"endobj")?;
                return Ok(trim_pdf(rest));
            }
            b"obj" | b"endobj" | b"endstream" | b"xref" | b"trailer" | b"startxref" => {
                return Err(Structure);
            }
            _ => {}
        }
    }
}

pub(super) fn reject_comments(mut bytes: &[u8]) -> VerifyResult<()> {
    while !trim_pdf(bytes).is_empty() {
        token(&mut bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_object_boundary_reports_workspace_budget() {
        let mut source = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for object in [
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
            "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>\nendobj\n",
        ] {
            offsets.push(source.len());
            source.extend_from_slice(object.as_bytes());
        }
        let xref = source.len();
        source.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
        for offset in offsets {
            source.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        source.extend_from_slice(
            format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        let reader = open_pdf_retained(&source, PdfReaderLimits::default(), MAX_RETAINED).unwrap();
        let root = PdfObjectRef {
            object_number: 1,
            generation: 0,
        };
        let (_, catalog_bytes, _) = reader
            .retained_object(root, MAX_RETAINED, MAX_DECODED, MAX_WORK)
            .unwrap();
        let overhead = Workspace::new(MAX_RETAINED);
        let mut reservation = overhead
            .reserve(128 + source.len() + reader.retained_cost)
            .unwrap();
        reservation
            .grow(4 * size_of::<(PdfObjectRef, Role)>())
            .unwrap();
        reservation
            .tree_entry::<(PdfObjectRef, Role, PdfValue)>()
            .unwrap();
        let allowance = MAX_RETAINED - overhead.remaining() + catalog_bytes - 1;
        let workspace = Workspace::new(allowance);
        let mut documents = workspace.scope();
        assert!(matches!(
            Document::open(&source, Side::Accepted, &workspace, &mut documents),
            Err(Budget)
        ));
        drop(documents);
        assert_eq!(workspace.remaining(), allowance);
    }

    #[test]
    fn coverage_reserves_evidence_while_documents_and_page_scratch_are_live() {
        use super::super::tests::redaction_fixture;
        use super::super::verifier::STANDALONE_WORKSPACE;

        let source = redaction_fixture("10 10 10 10 re f 40 40 10 10 re f");
        let result = redaction_fixture("40 40 10 10 re f");
        let workspace = Workspace::new(STANDALONE_WORKSPACE);
        let mut documents = workspace.scope();
        let accepted = Document::open(&source, Side::Accepted, &workspace, &mut documents).unwrap();
        let candidate =
            Document::open(&result, Side::Candidate, &workspace, &mut documents).unwrap();
        let remaining = workspace.remaining();
        let geometry = {
            let mut page_scratch = workspace.scope();
            accepted.page(0, &mut page_scratch).unwrap().geometry
        };
        let scratch = size_of::<PdfObjectRef>();
        let capacity = size_of::<NativeRedactionPageEvidence>();
        let marks = [NativeRedactionMark {
            page_id: 1,
            geometry,
            rect: [12_000, 12_000, 13_000, 13_000],
        }];
        let _limit = workspace.reserve(remaining - scratch - capacity).unwrap();
        {
            let _short = workspace.reserve(1).unwrap();
            let mut evidence = workspace.scope();
            assert_eq!(
                coverage(&accepted, &candidate, &[1], &marks, &mut evidence),
                Err(Budget)
            );
        }
        let mut evidence = workspace.scope();
        let covered = coverage(&accepted, &candidate, &[1], &marks, &mut evidence).unwrap();
        assert_eq!(
            covered,
            [NativeRedactionPageEvidence {
                page_id: 1,
                geometry
            }]
        );
        assert_eq!(workspace.remaining(), scratch);
    }

    #[test]
    fn exact_flate_reserves_and_releases_comparison_scratch() {
        let data = b"0 g 10 10 12 12 re f".to_vec();
        let encoded = miniz_oxide::deflate::compress_to_vec_zlib(&data, 6);
        let mut stream = PdfStream {
            dict: Dict::new(),
            data,
            encoded,
            source_offset: 0,
        };
        let scratch = stream.data.len() + 1 + size_of::<DecompressorOxide>();
        let constrained = Workspace::new(scratch - 1);
        assert_eq!(exact_flate(&stream, &constrained), Err(Budget));
        assert_eq!(constrained.remaining(), scratch - 1);

        let workspace = Workspace::new(scratch);
        assert_eq!(exact_flate(&stream, &workspace), Ok(()));
        assert_eq!(workspace.remaining(), scratch);
        stream.encoded[0] = 0;
        assert_eq!(exact_flate(&stream, &workspace), Err(Structure));
        assert_eq!(workspace.remaining(), scratch);
    }

    #[test]
    fn fit_to_profile_rejects_unknown_or_incremental_trailer_keys() {
        let mut dict = Dict::new();
        dict.insert("Size".into(), PdfValue::Integer(1));
        assert_eq!(keys(&dict, &["Size", "Root"]), Ok(()));
        for key in ["Prev", "Info", "Encrypt", "PrivateData"] {
            dict.insert(key.into(), PdfValue::Integer(1));
            assert_eq!(keys(&dict, &["Size", "Root"]), Err(Structure));
            dict.remove(key);
        }
    }
}
