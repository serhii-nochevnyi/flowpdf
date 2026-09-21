use flow_core::{
    MAX_IMAGE_DIMENSION, MAX_IMAGE_ENCODED_BYTES, RECORD_FORMAT_VERSION,
    asset::AssetError,
    canonical::asset_hash,
    layout::{FontCatalog, FontFace, LayoutRect, LayoutUnit, PaginationRequest, paginate_document},
    model::{
        AssetDescriptor, AssetId, BlockKind, ContentNode, FlowDocument, ImageAccessibility, NodeId,
    },
    pdf::{
        PdfError, PdfExportOptions, PdfExportRequest, PdfImageEncoding, PdfImageError,
        PdfInternalLink, PdfMetadataError, PdfMetadataOptions, PdfOutlineEntry, PdfPagePlan,
        PdfSupportedFeature, PdfUnsupportedFeature, build_display_list, build_image_resources,
        export_pdf,
    },
    store::AssetRecord,
};

const NOTO_SANS: &[u8] = include_bytes!("../data/NotoSans-Regular.ttf");

fn id(value: u32) -> NodeId {
    NodeId::new(format!("00000000-0000-4000-8000-{value:012x}")).unwrap()
}

fn asset_id(value: u32) -> AssetId {
    AssetId::new(format!("00000000-0000-4000-8000-{value:012x}")).unwrap()
}

fn one_pixel_png(pixel: [u8; 4]) -> Vec<u8> {
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&pixel, 1, 1, ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

fn one_pixel_jpeg() -> Vec<u8> {
    use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder};

    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 80)
        .write_image(&[20, 80, 160], 1, 1, ExtendedColorType::Rgb8)
        .unwrap();
    bytes
}

fn catalog() -> FontCatalog {
    FontCatalog::new(vec![
        FontFace::new("noto-sans", "Noto Sans", 0, NOTO_SANS.to_vec()).unwrap(),
    ])
    .unwrap()
}

fn document_with_assets(descriptors: Vec<AssetDescriptor>) -> FlowDocument {
    let mut document = FlowDocument::deterministic_sample("en-US").unwrap();
    document.content = descriptors
        .iter()
        .enumerate()
        .map(|(index, descriptor)| ContentNode {
            id: id(9_900 + u32::try_from(index).unwrap()),
            style_id: None,
            body: BlockKind::Image {
                asset_id: descriptor.id.clone(),
                accessibility: ImageAccessibility::Described {
                    text: format!("Image {}", index + 1),
                },
            },
        })
        .collect();
    document.assets = descriptors;
    document.fields.clear();
    document
}

fn descriptor(id: AssetId, bytes: &[u8], media_type: &str) -> AssetDescriptor {
    AssetDescriptor {
        id,
        content_hash: asset_hash(bytes),
        media_type: media_type.to_owned(),
        byte_length: u32::try_from(bytes.len()).unwrap(),
        alt_text: "An admitted image".to_owned(),
    }
}

fn record(bytes: Vec<u8>) -> AssetRecord {
    AssetRecord {
        record_format_version: RECORD_FORMAT_VERSION,
        content_hash: asset_hash(&bytes),
        bytes,
    }
}

#[test]
fn png_and_jpeg_resources_are_validated_deduplicated_and_deterministic() {
    let png = one_pixel_png([0, 120, 240, 128]);
    let jpeg = one_pixel_jpeg();
    let png_first = descriptor(asset_id(9_001), &png, "image/png");
    let mut png_alias = png_first.clone();
    png_alias.id = asset_id(9_002);
    let jpeg_descriptor = descriptor(asset_id(9_003), &jpeg, "image/jpeg");
    let document = document_with_assets(vec![png_first, png_alias, jpeg_descriptor]);
    let records = vec![record(png.clone()), record(jpeg.clone())];

    let first = build_image_resources(&document, &records).unwrap();
    let second = build_image_resources(&document, &records).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].resource_name, "Im1");
    assert_eq!(first[1].resource_name, "Im2");
    let png_resource = first
        .iter()
        .find(|resource| resource.media_type == "image/png")
        .unwrap();
    assert_eq!(png_resource.encoding, PdfImageEncoding::RawRgba);
    assert_eq!(png_resource.asset_ids.len(), 2);
    assert_eq!(png_resource.bytes, vec![0, 120, 240, 128]);
    let jpeg_resource = first
        .iter()
        .find(|resource| resource.media_type == "image/jpeg")
        .unwrap();
    assert_eq!(jpeg_resource.encoding, PdfImageEncoding::JpegDct);
    assert_eq!(jpeg_resource.bytes, jpeg);
    assert!(
        first
            .iter()
            .all(|resource| !resource.resource_identity.is_empty())
    );
}

#[test]
fn image_fragments_keep_source_asset_identity_and_fixed_rectangles() {
    let png = one_pixel_png([0, 0, 0, 255]);
    let image = descriptor(asset_id(9_010), &png, "image/png");
    let document = document_with_assets(vec![image.clone()]);
    let request = PaginationRequest::for_document(&document).unwrap();
    let result = paginate_document(&document, &request, &catalog(), None).unwrap();
    let display = build_display_list(&document, &result, &catalog(), None).unwrap();

    let item = display.pages[0].images.first().unwrap();
    assert_eq!(item.asset_id, image.id);
    assert_eq!(item.source_node_id, Some(document.content[0].id.clone()));
    assert!(item.rect.width.raw() > 0);
    assert!(item.rect.height.raw() > 0);
    assert!(display.diagnostics.is_empty());
}

#[test]
fn missing_hash_mismatch_and_duplicate_records_fail_closed_without_bytes_in_errors() {
    let png = one_pixel_png([0, 0, 0, 255]);
    let image = descriptor(asset_id(9_020), &png, "image/png");
    let document = document_with_assets(vec![image.clone()]);

    let missing = build_image_resources(&document, &[]).unwrap_err();
    assert_eq!(missing, PdfImageError::RecordMissing);

    let wrong_bytes = one_pixel_png([255, 0, 0, 255]);
    let mismatch = build_image_resources(&document, &[record(wrong_bytes.clone())]).unwrap_err();
    assert_eq!(mismatch, PdfImageError::RecordMissing);
    assert!(
        !mismatch
            .to_string()
            .contains(String::from_utf8_lossy(&wrong_bytes).as_ref())
    );

    let mut forged = record(wrong_bytes);
    forged.content_hash = image.content_hash.clone();
    let mismatch = build_image_resources(&document, &[forged]).unwrap_err();
    assert_eq!(mismatch, PdfImageError::ContentHashMismatch);

    let duplicate =
        build_image_resources(&document, &[record(png.clone()), record(png)]).unwrap_err();
    assert_eq!(duplicate, PdfImageError::DuplicateRecord);
}

#[test]
fn image_limits_are_reused_before_pdf_resource_creation() {
    use image::ImageEncoder;

    let oversized = vec![0_u8; MAX_IMAGE_ENCODED_BYTES + 1];
    let oversized_descriptor = AssetDescriptor {
        id: asset_id(9_030),
        content_hash: asset_hash(&oversized),
        media_type: "image/png".to_owned(),
        byte_length: u32::try_from(oversized.len()).unwrap(),
        alt_text: "Oversized".to_owned(),
    };
    let oversized_document = document_with_assets(vec![oversized_descriptor]);
    let error = build_image_resources(&oversized_document, &[record(oversized)]).unwrap_err();
    assert_eq!(error, PdfImageError::Asset(AssetError::EncodedSizeLimit));

    let pixels = vec![0_u8; usize::try_from(MAX_IMAGE_DIMENSION + 1).unwrap() * 4];
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &pixels,
            MAX_IMAGE_DIMENSION + 1,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let descriptor = descriptor(asset_id(9_031), &bytes, "image/png");
    let document = document_with_assets(vec![descriptor]);
    let error = build_image_resources(&document, &[record(bytes)]).unwrap_err();
    assert_eq!(error, PdfImageError::Asset(AssetError::DimensionLimit));
}

fn pdf_page() -> PdfPagePlan {
    PdfPagePlan::empty(
        LayoutUnit::from_raw(595 * 64),
        LayoutUnit::from_raw(842 * 64),
    )
    .unwrap()
}

#[test]
fn metadata_outlines_and_internal_links_are_deterministic_and_bounded() {
    let options = PdfExportOptions {
        metadata: PdfMetadataOptions {
            title: Some("Contract (draft)".to_owned()),
            author: Some("Олена".to_owned()),
            subject: Some("Internal navigation".to_owned()),
            language: Some("uk-UA".to_owned()),
        },
        outlines: vec![
            PdfOutlineEntry {
                id: "second".to_owned(),
                title: "Second".to_owned(),
                page_index: 1,
            },
            PdfOutlineEntry {
                id: "first".to_owned(),
                title: "First".to_owned(),
                page_index: 0,
            },
        ],
        internal_links: vec![PdfInternalLink {
            id: "jump-to-first".to_owned(),
            page_index: 1,
            rect: LayoutRect {
                x: LayoutUnit::from_raw(32),
                y: LayoutUnit::from_raw(64),
                width: LayoutUnit::from_raw(120 * 64),
                height: LayoutUnit::from_raw(24 * 64),
            },
            destination_page_index: 0,
        }],
        ..PdfExportOptions::default()
    };
    let request = PdfExportRequest::new(
        4,
        "source-hash",
        "layout-hash",
        vec![pdf_page(), pdf_page()],
        options.clone(),
    )
    .unwrap();
    let first = export_pdf(&request).unwrap();

    let mut reordered = options;
    reordered.outlines.reverse();
    let reordered_request = PdfExportRequest::new(
        4,
        "source-hash",
        "layout-hash",
        vec![pdf_page(), pdf_page()],
        reordered,
    )
    .unwrap();
    let second = export_pdf(&reordered_request).unwrap();
    assert_eq!(first.export_fingerprint, second.export_fingerprint);
    assert_eq!(first.bytes, second.bytes);

    let text = String::from_utf8_lossy(&first.bytes);
    assert!(text.contains("/Title (Contract \\(draft\\))"));
    assert!(text.contains("/Lang (uk-UA)"));
    assert!(text.contains("/Type /Outlines"));
    assert!(text.contains("/Subtype /Link"));
    assert!(text.find("(First)").unwrap() < text.find("(Second)").unwrap());
    for forbidden in [
        "/JavaScript",
        "/Launch",
        "/URI",
        "/ExternalFile",
        "/Encrypt",
    ] {
        assert!(
            !text.contains(forbidden),
            "forbidden PDF token: {forbidden}"
        );
    }
    assert_eq!(
        first.support_report.supported,
        vec![
            PdfSupportedFeature::Text,
            PdfSupportedFeature::Images,
            PdfSupportedFeature::Metadata,
            PdfSupportedFeature::Outlines,
            PdfSupportedFeature::InternalLinks,
        ]
    );
    assert!(
        first
            .support_report
            .unsupported
            .contains(&PdfUnsupportedFeature::JavaScript)
    );
    assert!(
        first
            .support_report
            .unsupported
            .contains(&PdfUnsupportedFeature::ExternalFiles)
    );
}

#[test]
fn metadata_and_links_reject_unrepresented_or_unsafe_structure() {
    let options = PdfExportOptions {
        metadata: PdfMetadataOptions {
            title: Some(String::new()),
            ..PdfMetadataOptions::default()
        },
        ..PdfExportOptions::default()
    };
    let error =
        PdfExportRequest::new(1, "source", "layout", vec![pdf_page()], options).unwrap_err();
    assert_eq!(error, PdfError::Metadata(PdfMetadataError::EmptyText));

    let options = PdfExportOptions {
        metadata: PdfMetadataOptions {
            language: Some("uk_UA".to_owned()),
            ..PdfMetadataOptions::default()
        },
        ..PdfExportOptions::default()
    };
    let error =
        PdfExportRequest::new(1, "source", "layout", vec![pdf_page()], options).unwrap_err();
    assert_eq!(error, PdfError::Metadata(PdfMetadataError::InvalidLanguage));

    let options = PdfExportOptions {
        internal_links: vec![PdfInternalLink {
            id: "external-looking".to_owned(),
            page_index: 0,
            rect: LayoutRect {
                x: LayoutUnit::from_raw(0),
                y: LayoutUnit::from_raw(0),
                width: LayoutUnit::from_raw(596 * 64),
                height: LayoutUnit::from_raw(10),
            },
            destination_page_index: 0,
        }],
        ..PdfExportOptions::default()
    };
    let error =
        PdfExportRequest::new(1, "source", "layout", vec![pdf_page()], options).unwrap_err();
    assert_eq!(error, PdfError::Metadata(PdfMetadataError::InvalidLinkRect));
}
