use std::path::PathBuf;

use uuid::Uuid;

/// Failure to read or decode a native workspace before graph compilation.
#[derive(Debug, thiserror::Error)]
pub enum AudienceWorkspaceLoadError {
    /// Workspace bytes could not be read.
    #[error("failed to read ProPresenter workspace at {path}: {source}")]
    Read {
        /// Exact configured workspace path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Configured workspace path is not a regular file.
    #[error("ProPresenter workspace path is not a regular file: {path}")]
    NotRegular {
        /// Exact configured workspace path.
        path: PathBuf,
    },
    /// Workspace bytes were not a valid native protobuf document.
    #[error("failed to decode ProPresenter workspace at {path}: {source}")]
    Decode {
        /// Exact configured workspace path.
        path: PathBuf,
        /// Protobuf decoding error.
        source: prost::DecodeError,
    },
}

/// Native identity kind used in precise workspace diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeIdentityKind {
    /// Saved Audience Look.
    AudienceLook,
    /// Screen reference carried by a saved Look.
    LookScreen,
    /// Theme slide selected for a screen.
    ThemeSlide,
    /// Audience Look identification carried by a macro action.
    MacroAudienceLook,
}

impl std::fmt::Display for NativeIdentityKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::AudienceLook => "Audience Look",
            Self::LookScreen => "Audience Look screen reference",
            Self::ThemeSlide => "theme slide",
            Self::MacroAudienceLook => "macro Audience Look reference",
        })
    }
}

/// Invalid native UUID with enough context for manual repair.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{kind} '{name}' has invalid UUID '{value}'")]
pub struct InvalidNativeIdentity {
    /// Semantic native identity kind.
    pub(crate) kind: NativeIdentityKind,
    /// Operator-visible owner name when available.
    pub(crate) name: String,
    /// Invalid native UUID text.
    pub(crate) value: String,
}

/// Failure while compiling the saved Look-to-screen-to-theme graph.
#[derive(Debug, thiserror::Error)]
pub enum AudienceWorkspaceError {
    /// A saved Audience Look has no native identity.
    #[error("saved Audience Look '{name}' has no UUID")]
    MissingLookUuid {
        /// Operator-visible Look name.
        name: String,
    },
    /// A Look's screen entry has no target identity.
    #[error("saved Audience Look '{look_name}' contains a screen entry with no screen UUID")]
    MissingLookScreenUuid {
        /// Operator-visible Look name.
        look_name: String,
    },
    /// A native UUID is malformed.
    #[error(transparent)]
    InvalidIdentity(#[from] InvalidNativeIdentity),
    /// Two logical screens claim the same UUID.
    #[error("ProPresenter screens '{first_name}' and '{duplicate_name}' share UUID {uuid}")]
    DuplicateScreenUuid {
        /// Conflicting UUID.
        uuid: Uuid,
        /// First screen name.
        first_name: String,
        /// Duplicate screen name.
        duplicate_name: String,
    },
    /// Two saved Looks claim the same UUID.
    #[error("saved Audience Looks '{first_name}' and '{duplicate_name}' share UUID {uuid}")]
    DuplicateLookUuid {
        /// Conflicting UUID.
        uuid: Uuid,
        /// First Look name.
        first_name: String,
        /// Duplicate Look name.
        duplicate_name: String,
    },
    /// A saved Look has multiple entries for the same logical screen.
    #[error("saved Audience Look '{look_name}' maps screen {screen_uuid} more than once")]
    DuplicateLookScreen {
        /// Operator-visible Look name.
        look_name: String,
        /// Duplicated logical screen UUID.
        screen_uuid: Uuid,
    },
    /// A saved Look refers to a logical screen absent from the workspace.
    #[error("saved Audience Look '{look_name}' refers to missing screen {screen_uuid}")]
    DanglingScreen {
        /// Operator-visible Look name.
        look_name: String,
        /// Missing logical screen UUID.
        screen_uuid: Uuid,
    },
    /// A screen carries an unknown native screen-type value.
    #[error("ProPresenter screen '{screen_name}' ({screen_uuid}) has unknown type {raw_type}")]
    UnknownScreenType {
        /// Native screen UUID.
        screen_uuid: Uuid,
        /// Operator-visible screen name.
        screen_name: String,
        /// Unknown protobuf enum value.
        raw_type: i32,
    },
    /// A theme override contains only half of its path/slide reference pair.
    #[error(
        "saved Audience Look '{look_name}' screen '{screen_name}' has an incomplete theme override (document={has_document}, slide={has_slide})"
    )]
    IncompleteThemeOverride {
        /// Operator-visible Look name.
        look_name: String,
        /// Operator-visible screen name.
        screen_name: String,
        /// Whether a theme document URL was present.
        has_document: bool,
        /// Whether a theme slide UUID was present.
        has_slide: bool,
    },
    /// Native theme URL had no usable local locator.
    #[error(
        "saved Audience Look '{look_name}' screen '{screen_name}' has an invalid theme document URL"
    )]
    InvalidThemeDocumentUrl {
        /// Operator-visible Look name.
        look_name: String,
        /// Operator-visible screen name.
        screen_name: String,
    },
    /// Theme URL has no local-file representation.
    #[error(
        "saved Audience Look '{look_name}' screen '{screen_name}' theme document cannot be resolved locally: {source_url}"
    )]
    UnresolvedThemeDocument {
        /// Operator-visible Look name.
        look_name: String,
        /// Operator-visible screen name.
        screen_name: String,
        /// Original native URL representation.
        source_url: String,
    },
    /// Theme URL resolved to a local path that does not exist.
    #[error(
        "saved Audience Look '{look_name}' screen '{screen_name}' theme document is missing: {path}"
    )]
    MissingThemeDocument {
        /// Operator-visible Look name.
        look_name: String,
        /// Operator-visible screen name.
        screen_name: String,
        /// Preferred checked local path.
        path: PathBuf,
    },
    /// Theme document bytes could not be read.
    #[error("failed to read Audience Look theme document at {path}: {source}")]
    ReadThemeDocument {
        /// Resolved theme document path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Theme document bytes were not valid native protobuf data.
    #[error("failed to decode Audience Look theme document at {path}: {source}")]
    DecodeThemeDocument {
        /// Resolved theme document path.
        path: PathBuf,
        /// Protobuf decoding error.
        source: prost::DecodeError,
    },
    /// A referenced theme slide does not exist in the resolved document.
    #[error(
        "saved Audience Look '{look_name}' screen '{screen_name}' refers to missing theme slide {slide_uuid} in {path}"
    )]
    DanglingThemeSlide {
        /// Operator-visible Look name.
        look_name: String,
        /// Operator-visible screen name.
        screen_name: String,
        /// Resolved theme document path.
        path: PathBuf,
        /// Missing native theme-slide UUID.
        slide_uuid: Uuid,
    },
    /// A resolved theme template has no slide body to render or measure.
    #[error(
        "saved Audience Look '{look_name}' screen '{screen_name}' theme slide {slide_uuid} has no base slide in {path}"
    )]
    MissingThemeBaseSlide {
        /// Operator-visible Look name.
        look_name: String,
        /// Operator-visible screen name.
        screen_name: String,
        /// Resolved theme document path.
        path: PathBuf,
        /// Referenced native theme-slide UUID.
        slide_uuid: Uuid,
    },
    /// A referenced theme slide UUID occurs more than once.
    #[error(
        "saved Audience Look '{look_name}' screen '{screen_name}' refers to ambiguous theme slide {slide_uuid} ({count} matches in {path})"
    )]
    AmbiguousThemeSlide {
        /// Operator-visible Look name.
        look_name: String,
        /// Operator-visible screen name.
        screen_name: String,
        /// Resolved theme document path.
        path: PathBuf,
        /// Duplicated native theme-slide UUID.
        slide_uuid: Uuid,
        /// Number of matching theme slides.
        count: usize,
    },
}

/// Failure to resolve the Audience Look selected by one installed macro.
#[derive(Debug, thiserror::Error)]
pub enum AudienceDestinationError {
    /// The selected saved Look's screen or theme graph is invalid.
    #[error(transparent)]
    Graph(#[from] AudienceWorkspaceError),
    /// No enabled Audience Look action exists.
    #[error("macro '{macro_name}' has no enabled Audience Look action")]
    MissingAudienceLookAction {
        /// Operator-visible macro name.
        macro_name: String,
    },
    /// An enabled action executes another macro, so the final Look depends on
    /// effects outside the selected macro.
    #[error(
        "macro '{macro_name}' has an enabled nested Macro action at index {action_index}; its final Audience Look cannot be proven"
    )]
    NestedMacroAction {
        /// Operator-visible containing macro name.
        macro_name: String,
        /// Zero-based native action index.
        action_index: usize,
    },
    /// Multiple enabled Audience Look actions make the result order-dependent.
    #[error("macro '{macro_name}' has {count} enabled Audience Look actions")]
    AmbiguousAudienceLookActions {
        /// Operator-visible macro name.
        macro_name: String,
        /// Conflicting enabled action count.
        count: usize,
    },
    /// Native action type claims Audience Look but carries no matching payload.
    #[error("macro '{macro_name}' Audience Look action has no Audience Look payload")]
    MissingAudienceLookActionData {
        /// Operator-visible macro name.
        macro_name: String,
    },
    /// Native Audience Look action has no collection identification.
    #[error("macro '{macro_name}' Audience Look action has no identification")]
    MissingAudienceLookIdentification {
        /// Operator-visible macro name.
        macro_name: String,
    },
    /// Macro Look identification has a name but no UUID.
    #[error("macro '{macro_name}' Audience Look '{look_name}' has no UUID")]
    MissingAudienceLookUuid {
        /// Operator-visible macro name.
        macro_name: String,
        /// Native target name.
        look_name: String,
    },
    /// Macro Look UUID is malformed.
    #[error(transparent)]
    InvalidIdentity(InvalidNativeIdentity),
    /// Macro Look UUID has no saved Look in the workspace.
    #[error("macro '{macro_name}' refers to missing Audience Look '{look_name}' ({look_uuid})")]
    DanglingAudienceLook {
        /// Operator-visible macro name.
        macro_name: String,
        /// Missing saved-Look UUID.
        look_uuid: Uuid,
        /// Native macro target name.
        look_name: String,
    },
    /// Macro target name contradicts the saved Look with the same UUID.
    #[error(
        "macro '{macro_name}' names Audience Look {look_uuid} as '{macro_look_name}', but the workspace names it '{workspace_look_name}'"
    )]
    AudienceLookNameMismatch {
        /// Operator-visible macro name.
        macro_name: String,
        /// Resolved saved-Look UUID.
        look_uuid: Uuid,
        /// Name carried by the macro action.
        macro_look_name: String,
        /// Name carried by the saved workspace Look.
        workspace_look_name: String,
    },
}
