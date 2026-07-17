#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;

use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use prost::Message;

use super::*;
use crate::paths::BuildLocationInputs;
use crate::project_config::{BackgroundAssetPath, BackgroundId, CueRoleConfig};
use crate::propresenter::package::PlaylistPackageMode;
use crate::propresenter::playlist::{playlist_output_path, PlaylistMediaAsset};
use crate::propresenter::theme::ThemeCacheLoadError;
use crate::workflow::description_parser::{ParsedContent, ParsedSegment, SpeakerRole};
use crate::workflow::plan::{CueMacro, OutputKey, RenderRole, ReviewContext};

mod support;
use support::*;

mod invariants;
mod native_fidelity;
mod output_safety;
mod overrides;
mod playlist_identity;
mod portable_export;
mod source_capture;
