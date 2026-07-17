//! Stable semantic summaries of native presentation structure.

/// Operator-facing presentation structure summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PresentationStructureSummary {
    /// Presentation UUID. Useful for duplicate-file detection; normally volatile
    /// for freshly generated documents.
    pub uuid: Option<String>,
    /// Presentation name.
    pub name: String,
    /// Scripture metadata attached to the presentation, when present.
    pub bible_reference: Option<BibleReferenceSummary>,
    /// Cue summaries in raw protobuf order.
    pub cues: Vec<CueStructureSummary>,
    /// Cue group summaries in protobuf order.
    pub cue_groups: Vec<CueGroupStructureSummary>,
    /// Arrangement summaries in protobuf order.
    pub arrangements: Vec<ArrangementStructureSummary>,
    /// Cue indexes in operator traversal order.
    pub operator_cue_indexes: Vec<usize>,
    /// Structural identity/reference problems that prevent lossless resolution.
    pub reference_diagnostics: Vec<PresentationReferenceDiagnostic>,
}

/// A malformed or ambiguous native presentation reference.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PresentationReferenceDiagnostic {
    /// More than one cue owns the same UUID.
    DuplicateCueUuid {
        /// Duplicated UUID value.
        uuid: String,
        /// Every raw cue index carrying the UUID.
        cue_indexes: Vec<usize>,
    },
    /// More than one cue group owns the same UUID.
    DuplicateCueGroupUuid {
        /// Duplicated UUID value.
        uuid: String,
        /// Every raw cue-group index carrying the UUID.
        cue_group_indexes: Vec<usize>,
    },
    /// More than one arrangement owns the same UUID.
    DuplicateArrangementUuid {
        /// Duplicated UUID value.
        uuid: String,
        /// Every raw arrangement index carrying the UUID.
        arrangement_indexes: Vec<usize>,
    },
    /// The selected arrangement UUID does not resolve to an arrangement.
    DanglingSelectedArrangement {
        /// Unresolved selected-arrangement UUID.
        uuid: String,
    },
    /// The selected arrangement UUID resolves to multiple arrangements.
    AmbiguousSelectedArrangement {
        /// Ambiguous selected-arrangement UUID.
        uuid: String,
        /// Every matching raw arrangement index.
        arrangement_indexes: Vec<usize>,
    },
    /// A cue-group reference does not resolve to a cue.
    DanglingCueReference {
        /// Raw cue-group index containing the reference.
        cue_group_index: usize,
        /// Reference position inside the cue group.
        reference_index: usize,
        /// Unresolved UUID value.
        uuid: String,
    },
    /// A cue-group reference resolves to multiple cues.
    AmbiguousCueReference {
        /// Raw cue-group index containing the reference.
        cue_group_index: usize,
        /// Reference position inside the cue group.
        reference_index: usize,
        /// Ambiguous UUID value.
        uuid: String,
        /// Every matching raw cue index.
        cue_indexes: Vec<usize>,
    },
    /// An arrangement reference does not resolve to a cue group.
    DanglingGroupReference {
        /// Raw arrangement index containing the reference.
        arrangement_index: usize,
        /// Reference position inside the arrangement.
        reference_index: usize,
        /// Unresolved UUID value.
        uuid: String,
    },
    /// An arrangement reference resolves to multiple cue groups.
    AmbiguousGroupReference {
        /// Raw arrangement index containing the reference.
        arrangement_index: usize,
        /// Reference position inside the arrangement.
        reference_index: usize,
        /// Ambiguous UUID value.
        uuid: String,
        /// Every matching raw cue-group index.
        cue_group_indexes: Vec<usize>,
    },
}

/// Cue-level semantic summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CueStructureSummary {
    /// Raw cue index.
    pub index: usize,
    /// Cue UUID.
    pub uuid: Option<String>,
    /// Cue name.
    pub name: String,
    /// Cue group names containing this cue.
    pub group_names: Vec<String>,
    /// Extracted slide text, preserving internal blank lines where possible.
    pub text: String,
    /// Extracted slide text split into lines.
    pub text_lines: Vec<String>,
    /// Whether the cue has no alphanumeric text.
    pub is_blank: bool,
    /// Macro action names on this cue, in action order.
    pub macros: Vec<String>,
    /// Labels attached to slide actions on this cue, in action order.
    pub slide_labels: Vec<ActionLabelSignature>,
    /// Background media basenames on this cue, in action order.
    pub background_media: Vec<String>,
    /// Action kind signature, in action order.
    pub action_kinds: Vec<String>,
    /// Slide text/layout style signatures, used as a proxy for theme/template parity.
    pub text_styles: Vec<TextStyleSignature>,
}

/// Label attached to a slide action.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActionLabelSignature {
    /// Operator-visible label text.
    pub text: String,
    /// Label color normalized to an RGBA hex string.
    pub color: Option<String>,
}

/// Presentation-level scripture metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BibleReferenceSummary {
    /// Native book index.
    pub book_index: u32,
    /// Operator-visible book name.
    pub book_name: String,
    /// Inclusive chapter range.
    pub chapter_range: Option<IntRangeSummary>,
    /// Inclusive verse range.
    pub verse_range: Option<IntRangeSummary>,
    /// Full translation name.
    pub translation_name: String,
    /// Translation abbreviation shown to the operator.
    pub translation_display_abbreviation: String,
    /// Translation abbreviation used internally by `ProPresenter`.
    pub translation_internal_abbreviation: String,
    /// Native book lookup key.
    pub book_key: String,
}

/// Inclusive integer range from native presentation metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct IntRangeSummary {
    /// First value in the range.
    pub start: i32,
    /// Last value in the range.
    pub end: i32,
}

/// Text element style/layout summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextStyleSignature {
    /// Text element name from the slide, when present.
    pub element_name: String,
    /// Element bounds in slide coordinates.
    pub bounds: Option<String>,
    /// Slide canvas size.
    pub slide_size: Option<String>,
    /// Font family/name resolved from attributes or RTF.
    pub font_name: Option<String>,
    /// Font size in points.
    pub font_size: Option<u32>,
    /// Text color in hex RGB/RGBA form.
    pub color: Option<String>,
    /// Bold style flag.
    pub bold: Option<bool>,
    /// Italic style flag.
    pub italic: Option<bool>,
    /// Vertical alignment enum name.
    pub vertical_alignment: String,
    /// Text scale behavior enum name.
    pub scale_behavior: String,
    /// Text transform enum name.
    pub transform: String,
    /// Text margins.
    pub margins: Option<String>,
}

/// Cue-group semantic summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CueGroupStructureSummary {
    /// Raw cue group index.
    pub index: usize,
    /// Cue group UUID.
    pub uuid: Option<String>,
    /// Cue group name.
    pub name: String,
    /// Group color normalized to an RGBA hex string.
    pub color: Option<String>,
    /// Keyboard shortcut bound to the group.
    pub hot_key: Option<HotKeySignature>,
    /// Identifier of the corresponding application-defined group.
    pub application_group_identifier: Option<String>,
    /// Name of the corresponding application-defined group.
    pub application_group_name: String,
    /// Cue indexes in this group.
    pub cue_indexes: Vec<usize>,
}

/// Keyboard shortcut attached to a cue group.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HotKeySignature {
    /// Native key-code value.
    pub code: i32,
    /// Native control identifier.
    pub control_identifier: String,
}

/// Arrangement semantic summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArrangementStructureSummary {
    /// Raw arrangement index.
    pub index: usize,
    /// Arrangement UUID.
    pub uuid: Option<String>,
    /// Arrangement name.
    pub name: String,
    /// Group names in arrangement order.
    pub group_names: Vec<String>,
    /// Cue indexes in arrangement traversal order.
    pub cue_indexes: Vec<usize>,
}
