//! Rust-owned command capability and modality-parity metadata.
//!
//! The editor has one semantic command bus.  Visible controls, keyboard
//! routes, and the future voice seam may translate physical input differently,
//! but they resolve to the same closed mutation family and therefore inherit
//! the same validation, confirmation, and undo behavior.

use serde::{Deserialize, Serialize};

use crate::transaction::{Command, CommandKind, Mutation, SourceModality};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MutationFamily {
    InsertText,
    ReplaceText,
    ReplaceSelection,
    DeleteText,
    SetNodeStyle,
    InsertNode,
    DeleteNode,
    SplitTextBlock,
    MergeTextBlocks,
    DeleteSubtree,
    SetInlineMarks,
    SetInlineMark,
    SetBlockAttributes,
    SetBlockStyle,
    SetListKind,
    ContinueListItem,
    ExitListItem,
    IndentListItem,
    OutdentListItem,
    InsertPageBreak,
    RemovePageBreak,
    InsertTable,
    InsertImage,
    ReplaceImage,
    SetImageAccessibility,
    RemoveImage,
    AddTableRow,
    RemoveTableRow,
    AddTableColumn,
    RemoveTableColumn,
    SetTableHeaderRow,
    RemoveTable,
    InsertField,
    SetField,
}

impl MutationFamily {
    pub const ALL: [Self; 34] = [
        Self::InsertText,
        Self::ReplaceText,
        Self::ReplaceSelection,
        Self::DeleteText,
        Self::SetNodeStyle,
        Self::InsertNode,
        Self::DeleteNode,
        Self::SplitTextBlock,
        Self::MergeTextBlocks,
        Self::DeleteSubtree,
        Self::SetInlineMarks,
        Self::SetInlineMark,
        Self::SetBlockAttributes,
        Self::SetBlockStyle,
        Self::SetListKind,
        Self::ContinueListItem,
        Self::ExitListItem,
        Self::IndentListItem,
        Self::OutdentListItem,
        Self::InsertPageBreak,
        Self::RemovePageBreak,
        Self::InsertTable,
        Self::InsertImage,
        Self::ReplaceImage,
        Self::SetImageAccessibility,
        Self::RemoveImage,
        Self::AddTableRow,
        Self::RemoveTableRow,
        Self::AddTableColumn,
        Self::RemoveTableColumn,
        Self::SetTableHeaderRow,
        Self::RemoveTable,
        Self::InsertField,
        Self::SetField,
    ];

    #[must_use]
    pub const fn command_type(self) -> &'static str {
        match self {
            Self::InsertText => "insertText",
            Self::ReplaceText => "replaceText",
            Self::ReplaceSelection => "replaceSelection",
            Self::DeleteText => "deleteText",
            Self::SetNodeStyle => "setNodeStyle",
            Self::InsertNode => "insertNode",
            Self::DeleteNode => "deleteNode",
            Self::SplitTextBlock => "splitTextBlock",
            Self::MergeTextBlocks => "mergeTextBlocks",
            Self::DeleteSubtree => "deleteSubtree",
            Self::SetInlineMarks => "setInlineMarks",
            Self::SetInlineMark => "setInlineMark",
            Self::SetBlockAttributes => "setBlockAttributes",
            Self::SetBlockStyle => "setBlockStyle",
            Self::SetListKind => "setListKind",
            Self::ContinueListItem => "continueListItem",
            Self::ExitListItem => "exitListItem",
            Self::IndentListItem => "indentListItem",
            Self::OutdentListItem => "outdentListItem",
            Self::InsertPageBreak => "insertPageBreak",
            Self::RemovePageBreak => "removePageBreak",
            Self::InsertTable => "insertTable",
            Self::InsertImage => "insertImage",
            Self::ReplaceImage => "replaceImage",
            Self::SetImageAccessibility => "setImageAccessibility",
            Self::RemoveImage => "removeImage",
            Self::AddTableRow => "addTableRow",
            Self::RemoveTableRow => "removeTableRow",
            Self::AddTableColumn => "addTableColumn",
            Self::RemoveTableColumn => "removeTableColumn",
            Self::SetTableHeaderRow => "setTableHeaderRow",
            Self::RemoveTable => "removeTable",
            Self::InsertField => "insertField",
            Self::SetField => "setField",
        }
    }

    #[must_use]
    pub const fn from_mutation(mutation: &Mutation) -> Self {
        match mutation {
            Mutation::InsertText { .. } => Self::InsertText,
            Mutation::ReplaceText { .. } => Self::ReplaceText,
            Mutation::ReplaceSelection { .. } => Self::ReplaceSelection,
            Mutation::DeleteText { .. } => Self::DeleteText,
            Mutation::SetNodeStyle { .. } => Self::SetNodeStyle,
            Mutation::InsertNode { .. } => Self::InsertNode,
            Mutation::DeleteNode { .. } => Self::DeleteNode,
            Mutation::SplitTextBlock { .. } => Self::SplitTextBlock,
            Mutation::MergeTextBlocks { .. } => Self::MergeTextBlocks,
            Mutation::DeleteSubtree { .. } => Self::DeleteSubtree,
            Mutation::SetInlineMarks { .. } => Self::SetInlineMarks,
            Mutation::SetInlineMark { .. } => Self::SetInlineMark,
            Mutation::SetBlockAttributes { .. } => Self::SetBlockAttributes,
            Mutation::SetBlockStyle { .. } => Self::SetBlockStyle,
            Mutation::SetListKind { .. } => Self::SetListKind,
            Mutation::ContinueListItem { .. } => Self::ContinueListItem,
            Mutation::ExitListItem { .. } => Self::ExitListItem,
            Mutation::IndentListItem { .. } => Self::IndentListItem,
            Mutation::OutdentListItem { .. } => Self::OutdentListItem,
            Mutation::InsertPageBreak { .. } => Self::InsertPageBreak,
            Mutation::RemovePageBreak { .. } => Self::RemovePageBreak,
            Mutation::InsertTable { .. } => Self::InsertTable,
            Mutation::InsertImage { .. } => Self::InsertImage,
            Mutation::ReplaceImage { .. } => Self::ReplaceImage,
            Mutation::SetImageAccessibility { .. } => Self::SetImageAccessibility,
            Mutation::RemoveImage { .. } => Self::RemoveImage,
            Mutation::AddTableRow { .. } => Self::AddTableRow,
            Mutation::RemoveTableRow { .. } => Self::RemoveTableRow,
            Mutation::AddTableColumn { .. } => Self::AddTableColumn,
            Mutation::RemoveTableColumn { .. } => Self::RemoveTableColumn,
            Mutation::SetTableHeaderRow { .. } => Self::SetTableHeaderRow,
            Mutation::RemoveTable { .. } => Self::RemoveTable,
            Mutation::InsertField { .. } => Self::InsertField,
            Mutation::SetField { .. } => Self::SetField,
        }
    }

    #[must_use]
    pub const fn from_command_kind(kind: &CommandKind) -> Option<Self> {
        match kind {
            CommandKind::InsertText { .. } => Some(Self::InsertText),
            CommandKind::ReplaceText { .. } => Some(Self::ReplaceText),
            CommandKind::ReplaceSelection { .. } => Some(Self::ReplaceSelection),
            CommandKind::DeleteText { .. } => Some(Self::DeleteText),
            CommandKind::SetNodeStyle { .. } => Some(Self::SetNodeStyle),
            CommandKind::InsertNode { .. } => Some(Self::InsertNode),
            CommandKind::DeleteNode { .. } => Some(Self::DeleteNode),
            CommandKind::SplitTextBlock { .. } => Some(Self::SplitTextBlock),
            CommandKind::MergeTextBlocks { .. } => Some(Self::MergeTextBlocks),
            CommandKind::DeleteSubtree { .. } => Some(Self::DeleteSubtree),
            CommandKind::SetInlineMarks { .. } => Some(Self::SetInlineMarks),
            CommandKind::SetInlineMark { .. } => Some(Self::SetInlineMark),
            CommandKind::SetBlockAttributes { .. } => Some(Self::SetBlockAttributes),
            CommandKind::SetBlockStyle { .. } => Some(Self::SetBlockStyle),
            CommandKind::SetListKind { .. } => Some(Self::SetListKind),
            CommandKind::ContinueListItem { .. } => Some(Self::ContinueListItem),
            CommandKind::ExitListItem { .. } => Some(Self::ExitListItem),
            CommandKind::IndentListItem { .. } => Some(Self::IndentListItem),
            CommandKind::OutdentListItem { .. } => Some(Self::OutdentListItem),
            CommandKind::InsertPageBreak { .. } => Some(Self::InsertPageBreak),
            CommandKind::RemovePageBreak { .. } => Some(Self::RemovePageBreak),
            CommandKind::InsertTable { .. } => Some(Self::InsertTable),
            CommandKind::InsertImage { .. } => Some(Self::InsertImage),
            CommandKind::ReplaceImage { .. } => Some(Self::ReplaceImage),
            CommandKind::SetImageAccessibility { .. } => Some(Self::SetImageAccessibility),
            CommandKind::RemoveImage { .. } => Some(Self::RemoveImage),
            CommandKind::AddTableRow { .. } => Some(Self::AddTableRow),
            CommandKind::RemoveTableRow { .. } => Some(Self::RemoveTableRow),
            CommandKind::AddTableColumn { .. } => Some(Self::AddTableColumn),
            CommandKind::RemoveTableColumn { .. } => Some(Self::RemoveTableColumn),
            CommandKind::SetTableHeaderRow { .. } => Some(Self::SetTableHeaderRow),
            CommandKind::RemoveTable { .. } => Some(Self::RemoveTable),
            CommandKind::InsertField { .. } => Some(Self::InsertField),
            CommandKind::SetField { .. } => Some(Self::SetField),
            CommandKind::Batch { .. } | CommandKind::Undo | CommandKind::Redo => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Text,
    Formatting,
    Structure,
    Asset,
    Compatibility,
    Destructive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConfirmationPolicy {
    None,
    Explicit,
    Conditional,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UndoPolicy {
    Reversible,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteBinding {
    pub label_key: &'static str,
    pub route: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FutureVoiceBinding {
    pub intent: &'static str,
    pub command_type: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandCapability {
    pub family: MutationFamily,
    pub command_type: &'static str,
    pub intent: &'static str,
    pub risk: RiskLevel,
    pub confirmation: ConfirmationPolicy,
    pub undo: UndoPolicy,
    pub visible: RouteBinding,
    pub keyboard: RouteBinding,
    pub future_voice: FutureVoiceBinding,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandParityContract {
    pub format_version: u32,
    pub phase: &'static str,
    pub mutation_count: usize,
    pub commands: &'static [CommandCapability],
}

#[allow(clippy::too_many_arguments)]
const fn capability(
    family: MutationFamily,
    intent: &'static str,
    risk: RiskLevel,
    confirmation: ConfirmationPolicy,
    visible_label_key: &'static str,
    visible_route: &'static str,
    keyboard_label_key: &'static str,
    keyboard_route: &'static str,
) -> CommandCapability {
    let command_type = family.command_type();
    CommandCapability {
        family,
        command_type,
        intent,
        risk,
        confirmation,
        undo: UndoPolicy::Reversible,
        visible: RouteBinding {
            label_key: visible_label_key,
            route: visible_route,
        },
        keyboard: RouteBinding {
            label_key: keyboard_label_key,
            route: keyboard_route,
        },
        future_voice: FutureVoiceBinding {
            intent,
            command_type,
        },
    }
}

pub static COMMAND_CAPABILITIES: [CommandCapability; 34] = [
    capability(
        MutationFamily::InsertText,
        "editor.intent.insertText",
        RiskLevel::Text,
        ConfirmationPolicy::None,
        "editor.parity.visible.insertText",
        "editor.inputHost",
        "editor.parity.keyboard.insertText",
        "keyboard.textInput",
    ),
    capability(
        MutationFamily::ReplaceText,
        "editor.intent.replaceText",
        RiskLevel::Text,
        ConfirmationPolicy::None,
        "editor.parity.visible.replaceText",
        "editor.document.selection",
        "editor.parity.keyboard.replaceText",
        "keyboard.textSelection",
    ),
    capability(
        MutationFamily::ReplaceSelection,
        "editor.intent.replaceSelection",
        RiskLevel::Text,
        ConfirmationPolicy::None,
        "editor.parity.visible.replaceSelection",
        "editor.inputHost",
        "editor.parity.keyboard.replaceSelection",
        "keyboard.textSelection",
    ),
    capability(
        MutationFamily::DeleteText,
        "editor.intent.deleteText",
        RiskLevel::Destructive,
        ConfirmationPolicy::None,
        "editor.parity.visible.deleteText",
        "editor.document.selection",
        "editor.parity.keyboard.deleteText",
        "keyboard.delete",
    ),
    capability(
        MutationFamily::SetNodeStyle,
        "editor.intent.setNodeStyle",
        RiskLevel::Formatting,
        ConfirmationPolicy::None,
        "editor.parity.visible.setNodeStyle",
        "editor.formatting.blockStyle",
        "editor.parity.keyboard.setNodeStyle",
        "keyboard.blockStyle",
    ),
    capability(
        MutationFamily::InsertNode,
        "editor.intent.insertNode",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.insertNode",
        "editor.insertMenu",
        "editor.parity.keyboard.insertNode",
        "keyboard.insertStructure",
    ),
    capability(
        MutationFamily::DeleteNode,
        "editor.intent.deleteNode",
        RiskLevel::Destructive,
        ConfirmationPolicy::Explicit,
        "editor.parity.visible.deleteNode",
        "editor.document.atomicActions",
        "editor.parity.keyboard.deleteNode",
        "keyboard.delete",
    ),
    capability(
        MutationFamily::SplitTextBlock,
        "editor.intent.splitTextBlock",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.splitTextBlock",
        "editor.structure",
        "editor.parity.keyboard.splitTextBlock",
        "keyboard.enter",
    ),
    capability(
        MutationFamily::MergeTextBlocks,
        "editor.intent.mergeTextBlocks",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.mergeTextBlocks",
        "editor.structure",
        "editor.parity.keyboard.mergeTextBlocks",
        "keyboard.backspace",
    ),
    capability(
        MutationFamily::DeleteSubtree,
        "editor.intent.deleteSubtree",
        RiskLevel::Destructive,
        ConfirmationPolicy::Explicit,
        "editor.parity.visible.deleteSubtree",
        "editor.document.atomicActions",
        "editor.parity.keyboard.deleteSubtree",
        "keyboard.delete",
    ),
    capability(
        MutationFamily::SetInlineMarks,
        "editor.intent.setInlineMarks",
        RiskLevel::Formatting,
        ConfirmationPolicy::None,
        "editor.parity.visible.setInlineMarks",
        "editor.toolbar.inlineMarks",
        "editor.parity.keyboard.setInlineMarks",
        "keyboard.modifierMarks",
    ),
    capability(
        MutationFamily::SetInlineMark,
        "editor.intent.setInlineMark",
        RiskLevel::Formatting,
        ConfirmationPolicy::None,
        "editor.parity.visible.setInlineMark",
        "editor.toolbar.inlineMarks",
        "editor.parity.keyboard.setInlineMark",
        "keyboard.modifierMarks",
    ),
    capability(
        MutationFamily::SetBlockAttributes,
        "editor.intent.setBlockAttributes",
        RiskLevel::Formatting,
        ConfirmationPolicy::None,
        "editor.parity.visible.setBlockAttributes",
        "editor.toolbar.blockAttributes",
        "editor.parity.keyboard.setBlockAttributes",
        "keyboard.blockAttributes",
    ),
    capability(
        MutationFamily::SetBlockStyle,
        "editor.intent.setBlockStyle",
        RiskLevel::Formatting,
        ConfirmationPolicy::None,
        "editor.parity.visible.setBlockStyle",
        "editor.toolbar.blockStyle",
        "editor.parity.keyboard.setBlockStyle",
        "keyboard.blockStyle",
    ),
    capability(
        MutationFamily::SetListKind,
        "editor.intent.setListKind",
        RiskLevel::Formatting,
        ConfirmationPolicy::None,
        "editor.parity.visible.setListKind",
        "editor.toolbar.list",
        "editor.parity.keyboard.setListKind",
        "keyboard.listShortcut",
    ),
    capability(
        MutationFamily::ContinueListItem,
        "editor.intent.continueListItem",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.continueListItem",
        "editor.listActions",
        "editor.parity.keyboard.continueListItem",
        "keyboard.enter",
    ),
    capability(
        MutationFamily::ExitListItem,
        "editor.intent.exitListItem",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.exitListItem",
        "editor.listActions",
        "editor.parity.keyboard.exitListItem",
        "keyboard.enter",
    ),
    capability(
        MutationFamily::IndentListItem,
        "editor.intent.indentListItem",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.indentListItem",
        "editor.listActions",
        "editor.parity.keyboard.indentListItem",
        "keyboard.tab",
    ),
    capability(
        MutationFamily::OutdentListItem,
        "editor.intent.outdentListItem",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.outdentListItem",
        "editor.listActions",
        "editor.parity.keyboard.outdentListItem",
        "keyboard.shiftTab",
    ),
    capability(
        MutationFamily::InsertPageBreak,
        "editor.intent.insertPageBreak",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.insertPageBreak",
        "editor.insertMenu",
        "editor.parity.keyboard.insertPageBreak",
        "keyboard.insertStructure",
    ),
    capability(
        MutationFamily::RemovePageBreak,
        "editor.intent.removePageBreak",
        RiskLevel::Destructive,
        ConfirmationPolicy::None,
        "editor.parity.visible.removePageBreak",
        "editor.document.atomicActions",
        "editor.parity.keyboard.removePageBreak",
        "keyboard.delete",
    ),
    capability(
        MutationFamily::InsertTable,
        "editor.intent.insertTable",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.insertTable",
        "editor.insertMenu",
        "editor.parity.keyboard.insertTable",
        "keyboard.insertStructure",
    ),
    capability(
        MutationFamily::InsertImage,
        "editor.intent.insertImage",
        RiskLevel::Asset,
        ConfirmationPolicy::None,
        "editor.parity.visible.insertImage",
        "editor.insertMenu",
        "editor.parity.keyboard.insertImage",
        "keyboard.insertStructure",
    ),
    capability(
        MutationFamily::ReplaceImage,
        "editor.intent.replaceImage",
        RiskLevel::Asset,
        ConfirmationPolicy::None,
        "editor.parity.visible.replaceImage",
        "editor.imageActions",
        "editor.parity.keyboard.replaceImage",
        "keyboard.replaceAtomic",
    ),
    capability(
        MutationFamily::SetImageAccessibility,
        "editor.intent.setImageAccessibility",
        RiskLevel::Asset,
        ConfirmationPolicy::None,
        "editor.parity.visible.setImageAccessibility",
        "editor.imageActions",
        "editor.parity.keyboard.setImageAccessibility",
        "keyboard.imageAccessibility",
    ),
    capability(
        MutationFamily::RemoveImage,
        "editor.intent.removeImage",
        RiskLevel::Destructive,
        ConfirmationPolicy::Explicit,
        "editor.parity.visible.removeImage",
        "editor.imageActions",
        "editor.parity.keyboard.removeImage",
        "keyboard.delete",
    ),
    capability(
        MutationFamily::AddTableRow,
        "editor.intent.addTableRow",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.addTableRow",
        "editor.tableActions",
        "editor.parity.keyboard.addTableRow",
        "keyboard.tableNavigation",
    ),
    capability(
        MutationFamily::RemoveTableRow,
        "editor.intent.removeTableRow",
        RiskLevel::Destructive,
        ConfirmationPolicy::Conditional,
        "editor.parity.visible.removeTableRow",
        "editor.tableActions",
        "editor.parity.keyboard.removeTableRow",
        "keyboard.tableNavigation",
    ),
    capability(
        MutationFamily::AddTableColumn,
        "editor.intent.addTableColumn",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.addTableColumn",
        "editor.tableActions",
        "editor.parity.keyboard.addTableColumn",
        "keyboard.tableNavigation",
    ),
    capability(
        MutationFamily::RemoveTableColumn,
        "editor.intent.removeTableColumn",
        RiskLevel::Destructive,
        ConfirmationPolicy::Conditional,
        "editor.parity.visible.removeTableColumn",
        "editor.tableActions",
        "editor.parity.keyboard.removeTableColumn",
        "keyboard.tableNavigation",
    ),
    capability(
        MutationFamily::SetTableHeaderRow,
        "editor.intent.setTableHeaderRow",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.setTableHeaderRow",
        "editor.tableActions",
        "editor.parity.keyboard.setTableHeaderRow",
        "keyboard.tableHeader",
    ),
    capability(
        MutationFamily::RemoveTable,
        "editor.intent.removeTable",
        RiskLevel::Destructive,
        ConfirmationPolicy::Explicit,
        "editor.parity.visible.removeTable",
        "editor.tableActions",
        "editor.parity.keyboard.removeTable",
        "keyboard.delete",
    ),
    capability(
        MutationFamily::InsertField,
        "editor.intent.insertField",
        RiskLevel::Structure,
        ConfirmationPolicy::None,
        "editor.parity.visible.insertField",
        "editor.insertMenu",
        "editor.parity.keyboard.insertField",
        "keyboard.insertStructure",
    ),
    capability(
        MutationFamily::SetField,
        "editor.intent.setField",
        RiskLevel::Compatibility,
        ConfirmationPolicy::None,
        "editor.parity.visible.setField",
        "foundationInspector.fieldReview",
        "editor.parity.keyboard.setField",
        "keyboard.fieldReview",
    ),
];

#[must_use]
pub const fn command_capabilities() -> &'static [CommandCapability] {
    &COMMAND_CAPABILITIES
}

#[must_use]
pub const fn command_capability_for_family(family: MutationFamily) -> &'static CommandCapability {
    &COMMAND_CAPABILITIES[family_index(family)]
}

#[must_use]
pub fn command_capability_for_command_kind(
    kind: &CommandKind,
) -> Option<&'static CommandCapability> {
    MutationFamily::from_command_kind(kind).map(command_capability_for_family)
}

#[must_use]
pub fn command_capability_for_voice_intent(intent: &str) -> Option<&'static CommandCapability> {
    COMMAND_CAPABILITIES
        .iter()
        .find(|capability| capability.future_voice.intent == intent)
}

#[must_use]
pub const fn command_parity_contract() -> CommandParityContract {
    CommandParityContract {
        format_version: 1,
        phase: "FLOWPDF-02-accessible-rich-text-editing",
        mutation_count: COMMAND_CAPABILITIES.len(),
        commands: &COMMAND_CAPABILITIES,
    }
}

/// Copy a validated command envelope while changing only its source modality.
/// Physical adapters use this seam for parity tests and future voice intent
/// translation; the command kind and all payload validation remain identical.
#[must_use]
pub fn command_with_modality(command: &Command, modality: SourceModality) -> Command {
    Command {
        modality,
        ..command.clone()
    }
}

const fn family_index(family: MutationFamily) -> usize {
    match family {
        MutationFamily::InsertText => 0,
        MutationFamily::ReplaceText => 1,
        MutationFamily::ReplaceSelection => 2,
        MutationFamily::DeleteText => 3,
        MutationFamily::SetNodeStyle => 4,
        MutationFamily::InsertNode => 5,
        MutationFamily::DeleteNode => 6,
        MutationFamily::SplitTextBlock => 7,
        MutationFamily::MergeTextBlocks => 8,
        MutationFamily::DeleteSubtree => 9,
        MutationFamily::SetInlineMarks => 10,
        MutationFamily::SetInlineMark => 11,
        MutationFamily::SetBlockAttributes => 12,
        MutationFamily::SetBlockStyle => 13,
        MutationFamily::SetListKind => 14,
        MutationFamily::ContinueListItem => 15,
        MutationFamily::ExitListItem => 16,
        MutationFamily::IndentListItem => 17,
        MutationFamily::OutdentListItem => 18,
        MutationFamily::InsertPageBreak => 19,
        MutationFamily::RemovePageBreak => 20,
        MutationFamily::InsertTable => 21,
        MutationFamily::InsertImage => 22,
        MutationFamily::ReplaceImage => 23,
        MutationFamily::SetImageAccessibility => 24,
        MutationFamily::RemoveImage => 25,
        MutationFamily::AddTableRow => 26,
        MutationFamily::RemoveTableRow => 27,
        MutationFamily::AddTableColumn => 28,
        MutationFamily::RemoveTableColumn => 29,
        MutationFamily::SetTableHeaderRow => 30,
        MutationFamily::RemoveTable => 31,
        MutationFamily::InsertField => 32,
        MutationFamily::SetField => 33,
    }
}
