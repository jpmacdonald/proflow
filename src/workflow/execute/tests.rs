#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;

use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use prost::Message;

use super::*;
use crate::paths::{BuildLocationInputs, BuildLocations};
use crate::project_config::ProjectConfig;
use crate::project_config::{BackgroundAssetPath, BackgroundId, CueRoleConfig};
use crate::workflow::description_parser::{ParsedContent, ParsedSegment, SpeakerRole};
use crate::workflow::plan::{
    CueMacro, OutputKey, PlanSemanticsError, RenderRole, ReviewContext, ScriptureContent,
};
use proflow_propresenter::playlist::{
    playlist_output_path, PlaylistExportIntent, PlaylistMediaAsset,
};
use proflow_propresenter::theme::ThemeCacheLoadError;
use proflow_propresenter::SlideType;

mod support;
use support::*;

mod background_files;
mod build_stages;
mod decision_options;
mod invariants;
mod library_freshness;
mod native_fidelity;
mod output_safety;
mod overrides;
mod playlist_identity;
mod portable_export;
mod receipt;
mod semantic_resolution;
mod source_capture;
