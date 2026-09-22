//! Rust-owned command-mode voice intent resolution.
//!
//! This module deliberately stops at a bounded, deterministic intent.  It
//! does not recognize audio, retain transcripts, mutate a document, or call a
//! browser API.  The web adapter may dispatch the returned action through the
//! existing editor/form seams only after checking the captured revision and
//! selection again.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::capability::{
    ConfirmationPolicy, MutationFamily, RiskLevel, UndoPolicy, command_capability_for_family,
};
use crate::editor_view::DirectionalSelection;
use crate::model::{FieldId, InlineMark, NodeId};

/// Version of the closed command-mode voice protocol.
pub const VOICE_PROTOCOL_VERSION: u32 = 1;
/// Maximum UTF-8 bytes admitted for one normalized speech transcript.
pub const MAX_VOICE_TRANSCRIPT_BYTES: usize = 4 * 1024;

/// Browser-recognition locales admitted by the command resolver.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoiceLocale {
    #[serde(rename = "uk-UA", alias = "uk")]
    UkUa,
    #[serde(rename = "en-US", alias = "en")]
    EnUs,
}

impl VoiceLocale {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UkUa => "uk-UA",
            Self::EnUs => "en-US",
        }
    }
}

/// The only request accepted by the command-mode resolver.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceCommandRequest {
    pub protocol_version: u32,
    pub locale: VoiceLocale,
    pub transcript: String,
    pub source_revision: u32,
    #[serde(default)]
    pub selection: Option<DirectionalSelection>,
    #[serde(default)]
    pub active_field_id: Option<FieldId>,
}

/// A closed action that a browser controller may translate to an existing
/// editor transaction or form-session action.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum VoiceAction {
    Undo,
    Redo,
    DeleteSelection,
    SetInlineMark { mark: InlineMark },
    InsertPageBreak,
    RemovePageBreak { page_break_id: NodeId },
    NavigateField { direction: FieldNavigationDirection },
    ClearField { field_id: FieldId },
}

/// Direction for the noncanonical field-navigation session action.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FieldNavigationDirection {
    Next,
    Previous,
}

/// Stable metadata projected from the existing command capability catalog.
/// `family` is absent only for history/navigation actions that are not
/// mutation families in the catalog.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceCapabilityMetadata {
    pub family: Option<MutationFamily>,
    pub command_type: String,
    pub intent: String,
    pub risk: RiskLevel,
    pub confirmation: ConfirmationPolicy,
    pub undo: Option<UndoPolicy>,
}

/// A revision/selection-bound command intent.  The transcript itself is
/// intentionally absent from this DTO and is never returned by the boundary.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceIntent {
    pub protocol_version: u32,
    pub locale: VoiceLocale,
    pub source_revision: u32,
    pub selection: Option<DirectionalSelection>,
    pub action: VoiceAction,
    pub capability: VoiceCapabilityMetadata,
}

/// Privacy-safe, stable resolver failures.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum VoiceError {
    #[error("the voice protocol version is unsupported")]
    ProtocolVersion,
    #[error("the voice transcript is empty")]
    TranscriptEmpty,
    #[error("the voice transcript exceeds the size limit")]
    TranscriptSizeLimit,
    #[error("the voice phrase is unsupported")]
    UnsupportedCommand,
    #[error("the voice phrase is ambiguous")]
    AmbiguousCommand,
    #[error("the voice action requires an editor selection")]
    SelectionRequired,
    #[error("the voice action requires an active field")]
    FieldRequired,
}

impl VoiceError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProtocolVersion => "FLOW_VOICE_PROTOCOL_UNSUPPORTED",
            Self::TranscriptEmpty => "FLOW_VOICE_TRANSCRIPT_EMPTY",
            Self::TranscriptSizeLimit => "FLOW_VOICE_TRANSCRIPT_SIZE_LIMIT",
            Self::UnsupportedCommand => "FLOW_VOICE_COMMAND_UNSUPPORTED",
            Self::AmbiguousCommand => "FLOW_VOICE_COMMAND_AMBIGUOUS",
            Self::SelectionRequired => "FLOW_VOICE_SELECTION_REQUIRED",
            Self::FieldRequired => "FLOW_VOICE_FIELD_REQUIRED",
        }
    }
}

/// Resolve one command-mode utterance without changing canonical document
/// state.  Matching is exact after bounded whitespace/case normalization and
/// is intentionally locale-specific.
pub fn resolve_voice_command(request: VoiceCommandRequest) -> Result<VoiceIntent, VoiceError> {
    if request.protocol_version != VOICE_PROTOCOL_VERSION {
        return Err(VoiceError::ProtocolVersion);
    }

    let phrase = normalize_transcript(&request.transcript)?;
    let action = match request.locale {
        VoiceLocale::EnUs => resolve_english(&phrase),
        VoiceLocale::UkUa => resolve_ukrainian(&phrase),
    }?;

    let capability = match &action {
        ResolvedAction::Undo => typed_capability("undo", "editor.intent.undo", RiskLevel::Text),
        ResolvedAction::Redo => typed_capability("redo", "editor.intent.redo", RiskLevel::Text),
        ResolvedAction::DeleteSelection => capability(
            MutationFamily::ReplaceSelection,
            Some(ConfirmationPolicy::Explicit),
        ),
        ResolvedAction::SetInlineMark { .. } => capability(MutationFamily::SetInlineMark, None),
        ResolvedAction::InsertPageBreak => capability(MutationFamily::InsertPageBreak, None),
        ResolvedAction::RemovePageBreak => capability(
            MutationFamily::RemovePageBreak,
            Some(ConfirmationPolicy::Explicit),
        ),
        ResolvedAction::NavigateField { .. } => typed_capability(
            "navigateField",
            "editor.intent.navigateField",
            RiskLevel::Text,
        ),
        ResolvedAction::ClearField => {
            capability(MutationFamily::SetField, Some(ConfirmationPolicy::Explicit))
        }
    };

    let requires_selection = matches!(
        &action,
        ResolvedAction::DeleteSelection
            | ResolvedAction::SetInlineMark { .. }
            | ResolvedAction::InsertPageBreak
            | ResolvedAction::RemovePageBreak
    );
    if requires_selection && request.selection.is_none() {
        return Err(VoiceError::SelectionRequired);
    }

    let action = match action {
        ResolvedAction::Undo => VoiceAction::Undo,
        ResolvedAction::Redo => VoiceAction::Redo,
        ResolvedAction::DeleteSelection => VoiceAction::DeleteSelection,
        ResolvedAction::SetInlineMark { mark } => VoiceAction::SetInlineMark { mark },
        ResolvedAction::InsertPageBreak => VoiceAction::InsertPageBreak,
        ResolvedAction::RemovePageBreak => {
            let selection = request
                .selection
                .as_ref()
                .ok_or(VoiceError::SelectionRequired)?;
            VoiceAction::RemovePageBreak {
                page_break_id: selection.anchor.node_id.clone(),
            }
        }
        ResolvedAction::NavigateField { direction } => VoiceAction::NavigateField { direction },
        ResolvedAction::ClearField => VoiceAction::ClearField {
            field_id: request.active_field_id.ok_or(VoiceError::FieldRequired)?,
        },
    };

    Ok(VoiceIntent {
        protocol_version: VOICE_PROTOCOL_VERSION,
        locale: request.locale,
        source_revision: request.source_revision,
        selection: request.selection,
        action,
        capability,
    })
}

fn normalize_transcript(transcript: &str) -> Result<String, VoiceError> {
    if transcript.len() > MAX_VOICE_TRANSCRIPT_BYTES {
        return Err(VoiceError::TranscriptSizeLimit);
    }
    let normalized = transcript
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if normalized.is_empty() {
        return Err(VoiceError::TranscriptEmpty);
    }
    if normalized.len() > MAX_VOICE_TRANSCRIPT_BYTES {
        return Err(VoiceError::TranscriptSizeLimit);
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedAction {
    Undo,
    Redo,
    DeleteSelection,
    SetInlineMark { mark: InlineMark },
    InsertPageBreak,
    RemovePageBreak,
    NavigateField { direction: FieldNavigationDirection },
    ClearField,
}

fn resolve_english(phrase: &str) -> Result<ResolvedAction, VoiceError> {
    match phrase {
        "undo" | "undo action" => Ok(ResolvedAction::Undo),
        "redo" | "redo action" => Ok(ResolvedAction::Redo),
        "delete selection" | "delete selected text" => Ok(ResolvedAction::DeleteSelection),
        "bold" | "make bold" => Ok(ResolvedAction::SetInlineMark {
            mark: InlineMark::Bold { value: true },
        }),
        "italic" | "make italic" => Ok(ResolvedAction::SetInlineMark {
            mark: InlineMark::Italic { value: true },
        }),
        "underline" | "make underline" => Ok(ResolvedAction::SetInlineMark {
            mark: InlineMark::Underline { value: true },
        }),
        "insert page break" => Ok(ResolvedAction::InsertPageBreak),
        "remove page break" | "delete page break" => Ok(ResolvedAction::RemovePageBreak),
        "next field" | "next form field" => Ok(ResolvedAction::NavigateField {
            direction: FieldNavigationDirection::Next,
        }),
        "previous field" | "previous form field" => Ok(ResolvedAction::NavigateField {
            direction: FieldNavigationDirection::Previous,
        }),
        "clear field" | "clear current field" => Ok(ResolvedAction::ClearField),
        "format" | "change formatting" => Err(VoiceError::AmbiguousCommand),
        _ => Err(VoiceError::UnsupportedCommand),
    }
}

fn resolve_ukrainian(phrase: &str) -> Result<ResolvedAction, VoiceError> {
    match phrase {
        "скасувати" | "скасувати дію" => Ok(ResolvedAction::Undo),
        "повторити" | "повторити дію" => Ok(ResolvedAction::Redo),
        "видалити виділене" | "видалити виділений текст" => {
            Ok(ResolvedAction::DeleteSelection)
        }
        "жирний" | "зробити жирним" => Ok(ResolvedAction::SetInlineMark {
            mark: InlineMark::Bold { value: true },
        }),
        "курсив" | "зробити курсивом" => Ok(ResolvedAction::SetInlineMark {
            mark: InlineMark::Italic { value: true },
        }),
        "підкреслити" | "зробити підкресленим" => {
            Ok(ResolvedAction::SetInlineMark {
                mark: InlineMark::Underline { value: true },
            })
        }
        "вставити розрив сторінки" => Ok(ResolvedAction::InsertPageBreak),
        "видалити розрив сторінки" | "прибрати розрив сторінки" => {
            Ok(ResolvedAction::RemovePageBreak)
        }
        "наступне поле" | "наступне поле форми" => {
            Ok(ResolvedAction::NavigateField {
                direction: FieldNavigationDirection::Next,
            })
        }
        "попереднє поле" | "попереднє поле форми" => {
            Ok(ResolvedAction::NavigateField {
                direction: FieldNavigationDirection::Previous,
            })
        }
        "очистити поле" | "очистити поточне поле" => {
            Ok(ResolvedAction::ClearField)
        }
        "форматувати" | "змінити форматування" => {
            Err(VoiceError::AmbiguousCommand)
        }
        _ => Err(VoiceError::UnsupportedCommand),
    }
}

fn capability(
    family: MutationFamily,
    confirmation_override: Option<ConfirmationPolicy>,
) -> VoiceCapabilityMetadata {
    let source = command_capability_for_family(family);
    VoiceCapabilityMetadata {
        family: Some(source.family),
        command_type: source.command_type.to_owned(),
        intent: source.intent.to_owned(),
        risk: source.risk,
        confirmation: confirmation_override.unwrap_or(source.confirmation),
        undo: Some(source.undo),
    }
}

fn typed_capability(command_type: &str, intent: &str, risk: RiskLevel) -> VoiceCapabilityMetadata {
    VoiceCapabilityMetadata {
        family: None,
        command_type: command_type.to_owned(),
        intent: intent.to_owned(),
        risk,
        confirmation: ConfirmationPolicy::None,
        undo: None,
    }
}
