use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost::Message;
use uuid::Uuid;

use crate::propresenter::generated::rv_data::{self, template, ProPresenterWorkspace};

/// One checked destination selected by an Audience Look for an audience screen.
#[derive(Clone, Debug, PartialEq)]
pub struct AudienceScreenDestination {
    pub(super) screen_uuid: Uuid,
    pub(super) screen_name: String,
    pub(super) presentation: PresentationDestination,
}

impl AudienceScreenDestination {
    /// Native identity of the logical audience screen.
    pub const fn screen_uuid(&self) -> Uuid {
        self.screen_uuid
    }

    /// Operator-visible audience-screen name.
    pub fn screen_name(&self) -> &str {
        &self.screen_name
    }

    /// Presentation foreground used by this screen.
    pub const fn presentation(&self) -> &PresentationDestination {
        &self.presentation
    }
}

/// The presentation foreground that `ProPresenter` renders on one screen.
#[derive(Clone, Debug, PartialEq)]
pub enum PresentationDestination {
    /// The Look keeps the presentation's own foreground styling.
    SourcePresentation,
    /// The Look restyles the foreground with one exact installed theme slide.
    ThemeOverride(Box<ThemeDestination>),
}

/// One validated theme document and slide selected by an Audience Look.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeDestination {
    pub(super) document_path: PathBuf,
    pub(super) document_sha256: [u8; 32],
    pub(super) slide_uuid: Uuid,
    pub(super) template: template::Slide,
    pub(super) base_slide: rv_data::Slide,
    pub(super) template_bytes: Vec<u8>,
}

impl ThemeDestination {
    /// Resolved local path of the native theme document.
    pub fn document_path(&self) -> &Path {
        &self.document_path
    }

    /// SHA-256 of the exact native theme document that supplied this template.
    pub const fn document_sha256(&self) -> [u8; 32] {
        self.document_sha256
    }

    /// Native UUID of the checked theme slide.
    pub const fn slide_uuid(&self) -> Uuid {
        self.slide_uuid
    }

    /// Exact decoded native template selected by UUID from the read document.
    pub const fn template(&self) -> &template::Slide {
        &self.template
    }

    /// Canonical protobuf bytes of the exact template selected by UUID.
    ///
    /// These bytes are captured while the document is read so downstream text
    /// fitting never reopens the file or resolves a slide by name.
    pub fn template_bytes(&self) -> &[u8] {
        &self.template_bytes
    }

    /// Exact destination base slide used for native text geometry.
    pub const fn base_slide(&self) -> &rv_data::Slide {
        &self.base_slide
    }
}

/// A saved Audience Look with all presentation destinations compiled.
#[derive(Clone, Debug, PartialEq)]
pub struct AudienceLookDestinations {
    pub(super) uuid: Uuid,
    pub(super) name: String,
    pub(super) screens: Vec<AudienceScreenDestination>,
}

impl AudienceLookDestinations {
    /// Native saved-Look UUID targeted by a macro action.
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Operator-visible saved-Look name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Audience screens whose presentation foreground is enabled.
    ///
    /// An empty slice is valid: a Look may intentionally hide presentation
    /// foregrounds on every audience screen.
    pub fn screens(&self) -> &[AudienceScreenDestination] {
        &self.screens
    }
}

/// Immutable resolver compiled from one native `ProPresenter` workspace.
#[derive(Debug)]
pub struct AudienceDestinationResolver {
    pub(super) workspace: ProPresenterWorkspace,
    pub(super) show_root: PathBuf,
    pub(super) themes: HashMap<PathBuf, ThemeDocument>,
    pub(super) source_path: Option<PathBuf>,
    pub(super) source_sha256: Option<[u8; 32]>,
}

#[derive(Debug)]
pub(super) struct ThemeDocument {
    pub(super) source_sha256: [u8; 32],
    pub(super) templates: HashMap<Uuid, Vec<ResolvedThemeTemplate>>,
}

#[derive(Clone, Debug)]
pub(super) struct ResolvedThemeTemplate {
    pub(super) native: template::Slide,
    pub(super) bytes: Vec<u8>,
}

impl ResolvedThemeTemplate {
    pub(super) fn new(native: template::Slide) -> Self {
        let bytes = native.encode_to_vec();
        Self { native, bytes }
    }
}

impl AudienceDestinationResolver {
    /// Exact native workspace document parsed by this resolver, when file-backed.
    pub(crate) fn source_document(&self) -> Option<(&Path, [u8; 32])> {
        self.source_path.as_deref().zip(self.source_sha256)
    }

    /// Theme documents parsed while resolving configured macro destinations.
    ///
    /// Callers sort these records before exposing them. The resolver cache is
    /// deliberately path-keyed so each exact document is read at most once.
    pub(crate) fn theme_documents(&self) -> impl Iterator<Item = (&Path, [u8; 32])> {
        self.themes
            .iter()
            .map(|(path, document)| (path.as_path(), document.source_sha256))
    }
}
