use flow_core::model::ImageAccessibility;
use flow_core::{
    ApiResponse, ApplyCommandRequest, AssetStageRequest, AssetStagingStore, CommandDto,
    CommandKind, CreateSampleRequest, MAX_IMAGE_ENCODED_BYTES, MAX_STAGING_RECEIPTS_PER_SESSION,
    OperationResult, STAGING_RECEIPT_TTL_SECONDS, SourceModality, StructuralPlacement,
    apply_command, create_sample, recover, stage_asset,
};
use flow_core::{
    canonical::decode_canonical,
    model::{CommandId, ContentNode, FlowDocument, NodeId},
    store::{CommitPlanner, DocumentStore, InMemoryDocumentStore, SnapshotPolicy, SnapshotReason},
    transaction::semantic_hash,
};

fn one_pixel_png() -> Vec<u8> {
    one_pixel_png_with([0, 0, 0, 255])
}

fn one_pixel_png_with(pixel: [u8; 4]) -> Vec<u8> {
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&pixel, 1, 1, ExtendedColorType::Rgba8)
        .expect("encode one-pixel PNG");
    bytes
}

fn stage_request(result: &OperationResult, session_id: &str) -> AssetStageRequest {
    AssetStageRequest {
        session_id: session_id.to_owned(),
        document_id: result.session.document_id.clone(),
        revision: result.session.revision,
    }
}

fn success<T>(response: ApiResponse<T>) -> T {
    assert!(response.ok, "expected success, got {:?}", response.error);
    response.value.expect("success value")
}

fn created() -> OperationResult {
    success(create_sample(CreateSampleRequest {
        requested_locale: "uk-UA".to_owned(),
        issued_at: "2026-09-14T12:00:00Z".to_owned(),
    }))
}

#[test]
fn staging_is_bounded_canonical_and_one_use() {
    let document = created();
    let request = stage_request(&document, "asset-test-session");
    let png = one_pixel_png();
    let mut store = AssetStagingStore::new();
    let response = store
        .stage_at(&request, png.clone(), 100)
        .expect("valid PNG stages");
    assert_eq!(response.media_type, "image/png");
    assert_eq!(response.width, 1);
    assert_eq!(response.height, 1);
    assert_eq!(
        response.content_hash,
        flow_core::canonical::asset_hash(&png)
    );
    assert!(response.content_hash.starts_with("blake3:"));
    assert!(
        !serde_json::to_string(&response)
            .expect("stage response serializes")
            .contains("staging_digest")
    );

    let redeemed = store
        .redeem_at(
            &response.receipt,
            &request.session_id,
            &request.document_id,
            request.revision,
            101,
        )
        .expect("receipt redeems once");
    assert_eq!(redeemed.into_record().bytes, png);
    let replay = store
        .redeem_at(
            &response.receipt,
            &request.session_id,
            &request.document_id,
            request.revision,
            102,
        )
        .expect_err("receipt replay is rejected");
    assert_eq!(replay.code(), "FLOW_ASSET_RECEIPT_REPLAYED");

    let expired = store
        .stage_at(&request, one_pixel_png(), 200)
        .expect("second stage");
    let error = store
        .redeem_at(
            &expired.receipt,
            &request.session_id,
            &request.document_id,
            request.revision,
            200 + STAGING_RECEIPT_TTL_SECONDS,
        )
        .expect_err("expired receipt is rejected");
    assert_eq!(error.code(), "FLOW_ASSET_RECEIPT_EXPIRED");
}

#[test]
fn staging_rejects_signature_size_and_session_overflow() {
    let document = created();
    let request = stage_request(&document, "asset-limit-session");
    let png = one_pixel_png();
    let mut store = AssetStagingStore::new();
    let format_error = store
        .stage_at(&request, b"not-an-image".to_vec(), 1)
        .expect_err("unknown signature rejected");
    assert_eq!(format_error.code(), "FLOW_ASSET_FORMAT_UNSUPPORTED");

    let size_error = store
        .stage_at(&request, vec![0; MAX_IMAGE_ENCODED_BYTES + 1], 1)
        .expect_err("encoded limit checked before decode");
    assert_eq!(size_error.code(), "FLOW_LIMIT_ASSET_ENCODED_BYTES");

    for _ in 0..MAX_STAGING_RECEIPTS_PER_SESSION {
        store
            .stage_at(&request, png.clone(), 1)
            .expect("receipt budget accepts the configured maximum");
    }
    let receipt_error = store
        .stage_at(&request, png.clone(), 1)
        .expect_err("third receipt is rejected");
    assert_eq!(receipt_error.code(), "FLOW_LIMIT_ASSET_RECEIPTS");
}

#[test]
fn insert_image_redeems_receipt_without_leaking_it_into_semantics() {
    let document = created();
    let session_id = "insert-image-session";
    let png = one_pixel_png();
    let stage = success(stage_asset(
        stage_request(&document, session_id),
        png.clone(),
    ));
    let command_id = flow_core::model::CommandId::new("00000000-0000-4000-8000-000000000901")
        .expect("command id");
    let applied = success(apply_command(ApplyCommandRequest {
        canonical_json: document.session.canonical_json,
        history: document.session.history,
        command: CommandDto {
            command_id,
            base_revision: document.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-14T12:00:01Z".to_owned(),
            kind: CommandKind::InsertImage {
                placement: StructuralPlacement {
                    parent_id: None,
                    index: 1,
                },
                session_id: session_id.to_owned(),
                receipt: stage.receipt.clone(),
                accessibility: ImageAccessibility::Described {
                    text: "Тестове зображення".to_owned(),
                },
            },
        },
    }));
    let canonical = &applied.session.canonical_json;
    assert!(!canonical.contains(&stage.receipt));
    assert!(!canonical.contains("staging_digest"));
    assert!(
        !serde_json::to_string(&applied.commit.transaction)
            .expect("transaction serializes")
            .contains(&stage.receipt)
    );
    assert_eq!(
        applied.commit.assets.len(),
        document.commit.assets.len() + 1
    );
    assert!(applied.commit.assets.iter().any(|asset| asset.bytes == png));

    let replay = apply_command(ApplyCommandRequest {
        canonical_json: applied.session.canonical_json,
        history: applied.session.history,
        command: CommandDto {
            command_id: flow_core::model::CommandId::new("00000000-0000-4000-8000-000000000902")
                .expect("replay command id"),
            base_revision: applied.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-14T12:00:02Z".to_owned(),
            kind: CommandKind::InsertImage {
                placement: StructuralPlacement {
                    parent_id: None,
                    index: 1,
                },
                session_id: session_id.to_owned(),
                receipt: stage.receipt,
                accessibility: ImageAccessibility::Decorative,
            },
        },
    });
    assert!(!replay.ok);
    assert_eq!(
        replay.error.expect("replay error").code,
        "FLOW_ASSET_RECEIPT_REPLAYED"
    );
}

#[test]
fn missing_legacy_is_never_accepted_as_a_new_accessibility_decision() {
    let error = flow_core::asset::validate_accessibility(&ImageAccessibility::MissingLegacy)
        .expect_err("legacy review state requires explicit resolution");
    assert_eq!(error.code(), "FLOW_ASSET_ACCESSIBILITY_REQUIRED");
}

fn command_id(serial: u32) -> CommandId {
    CommandId::new(format!("00000000-0000-4000-8000-{serial:012}")).expect("test command id")
}

fn apply_kind(result: OperationResult, serial: u32, kind: CommandKind) -> OperationResult {
    success(apply_command(ApplyCommandRequest {
        canonical_json: result.session.canonical_json,
        history: result.session.history,
        command: CommandDto {
            command_id: command_id(serial),
            base_revision: result.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-14T12:01:00Z".to_owned(),
            kind,
        },
    }))
}

fn find_image(
    nodes: &[ContentNode],
    content_hash: &str,
    document: &FlowDocument,
) -> Option<NodeId> {
    for node in nodes {
        if let flow_core::model::BlockKind::Image { asset_id, .. } = &node.body
            && document
                .assets
                .iter()
                .any(|asset| asset.id == *asset_id && asset.content_hash == content_hash)
        {
            return Some(node.id.clone());
        }
        if let Some(found) = find_image(node.children(), content_hash, document) {
            return Some(found);
        }
    }
    None
}

#[test]
fn image_lifecycle_is_exactly_reversible_and_recoverable() {
    let created_result = created();
    let session_id = "lifecycle-session";
    let first_bytes = one_pixel_png();
    let first_stage = success(stage_asset(
        stage_request(&created_result, session_id),
        first_bytes.clone(),
    ));
    let inserted = apply_kind(
        created_result.clone(),
        910,
        CommandKind::InsertImage {
            placement: StructuralPlacement {
                parent_id: None,
                index: 1,
            },
            session_id: session_id.to_owned(),
            receipt: first_stage.receipt,
            accessibility: ImageAccessibility::Described {
                text: "Початкове зображення".to_owned(),
            },
        },
    );
    let inserted_document =
        decode_canonical(inserted.session.canonical_json.as_bytes()).expect("inserted document");
    let image_id = find_image(
        &inserted_document.content,
        &first_stage.content_hash,
        &inserted_document,
    )
    .expect("inserted image node");

    let described = apply_kind(
        inserted.clone(),
        911,
        CommandKind::SetImageAccessibility {
            image_node_id: image_id.clone(),
            accessibility: ImageAccessibility::Described {
                text: "Оновлений опис українською".to_owned(),
            },
        },
    );
    let described_document =
        decode_canonical(described.session.canonical_json.as_bytes()).expect("described document");
    assert!(
        serde_json::to_string(&described_document)
            .expect("document serializes")
            .contains("Оновлений опис українською")
    );

    let replacement_bytes = one_pixel_png_with([255, 0, 0, 255]);
    let replacement_stage = success(stage_asset(
        AssetStageRequest {
            session_id: session_id.to_owned(),
            document_id: described.session.document_id.clone(),
            revision: described.session.revision,
        },
        replacement_bytes.clone(),
    ));
    let replaced = apply_kind(
        described.clone(),
        912,
        CommandKind::ReplaceImage {
            image_node_id: image_id.clone(),
            session_id: session_id.to_owned(),
            receipt: replacement_stage.receipt,
            accessibility: ImageAccessibility::Decorative,
        },
    );
    let replaced_document =
        decode_canonical(replaced.session.canonical_json.as_bytes()).expect("replaced document");
    let replaced_image = find_image(
        &replaced_document.content,
        &replacement_stage.content_hash,
        &replaced_document,
    )
    .expect("replacement keeps the image node");
    assert_eq!(replaced_image, image_id);

    let unconfirmed = apply_command(ApplyCommandRequest {
        canonical_json: replaced.session.canonical_json.clone(),
        history: replaced.session.history.clone(),
        command: CommandDto {
            command_id: command_id(913),
            base_revision: replaced.session.revision,
            modality: SourceModality::Ui,
            issued_at: "2026-09-14T12:01:00Z".to_owned(),
            kind: CommandKind::RemoveImage {
                image_node_id: image_id.clone(),
                confirmed: false,
            },
        },
    });
    assert!(!unconfirmed.ok);
    assert_eq!(
        unconfirmed.error.expect("confirmation error").code,
        "FLOW_CONFIRMATION_REQUIRED"
    );

    let removed = apply_kind(
        replaced.clone(),
        914,
        CommandKind::RemoveImage {
            image_node_id: image_id,
            confirmed: true,
        },
    );
    let removed_document =
        decode_canonical(removed.session.canonical_json.as_bytes()).expect("removed document");
    assert!(
        find_image(
            &removed_document.content,
            &replacement_stage.content_hash,
            &removed_document,
        )
        .is_none()
    );

    let undone = apply_kind(removed.clone(), 915, CommandKind::Undo);
    let undone_document =
        decode_canonical(undone.session.canonical_json.as_bytes()).expect("undone document");
    assert_eq!(
        semantic_hash(&undone_document).expect("undo semantic hash"),
        semantic_hash(&replaced_document).expect("replacement semantic hash")
    );
    let redone = apply_kind(undone.clone(), 916, CommandKind::Redo);
    let redone_document =
        decode_canonical(redone.session.canonical_json.as_bytes()).expect("redone document");
    assert_eq!(
        semantic_hash(&redone_document).expect("redo semantic hash"),
        semantic_hash(&removed_document).expect("removed semantic hash")
    );

    let mut store = InMemoryDocumentStore::default();
    let planner = CommitPlanner::new(SnapshotPolicy::EveryTransaction);
    planner
        .commit(&mut store, created_result.commit, SnapshotReason::Creation)
        .expect("create commit");
    let mut candidate = store.load_records().expect("records before insert");
    candidate.snapshots.push(inserted.commit.snapshot.clone());
    candidate
        .transactions
        .push(inserted.commit.transaction.clone());
    candidate.audits.push(inserted.commit.audit.clone());
    for asset in &inserted.commit.assets {
        if !candidate
            .assets
            .iter()
            .any(|existing| existing.content_hash == asset.content_hash)
        {
            candidate.assets.push(asset.clone());
        }
    }
    let candidate_recovery = recover(candidate);
    assert!(
        candidate_recovery.ok,
        "candidate recovery: {:?}",
        candidate_recovery.error
    );
    let insert_plan = planner.plan(
        &store.load_records().expect("records before insert"),
        inserted.commit.clone(),
        SnapshotReason::CommittedTransaction,
    );
    assert!(insert_plan.is_ok(), "insert plan: {insert_plan:?}");
    planner
        .commit(
            &mut store,
            inserted.commit,
            SnapshotReason::CommittedTransaction,
        )
        .expect("insert commit");
    planner
        .commit(
            &mut store,
            described.commit,
            SnapshotReason::CommittedTransaction,
        )
        .expect("accessibility commit keeps durable bytes");
    planner
        .commit(
            &mut store,
            replaced.commit,
            SnapshotReason::CommittedTransaction,
        )
        .expect("replacement commit keeps old and new bytes");
    planner
        .commit(
            &mut store,
            removed.commit,
            SnapshotReason::CommittedTransaction,
        )
        .expect("remove commit does not collect shared bytes");
    let durable = store.load_records().expect("durable records");
    assert!(
        durable
            .assets
            .iter()
            .any(|asset| asset.bytes == first_bytes)
    );
    assert!(
        durable
            .assets
            .iter()
            .any(|asset| asset.bytes == replacement_bytes)
    );
    let recovered = success(recover(durable));
    assert_eq!(
        recovered.session.canonical_json,
        removed.session.canonical_json
    );
}
