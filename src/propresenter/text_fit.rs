//! Checked client for native macOS text measurement.
//!
//! ProPresenter uses Apple's text stack for slide layout. This module keeps the
//! platform boundary deliberately small: Rust owns request validation and
//! evidence validation, while a persistent JSON-lines helper owns AppKit
//! shaping and layout. The helper never decides whether a slide is readable;
//! it reports physical evidence for higher-level policy to evaluate.

mod client;
mod evidence;
mod font_freshness;
mod native;
mod request;
mod wire;

pub(crate) use client::{NativeTextFitOracle, TextFitError};
pub(crate) use evidence::TextFitEvidence;
pub use evidence::{
    AudienceTextRendering, CueTextFitSummary, NativeLayoutRuntimeSummary, ResolvedFontSummary,
    TextFitContractSummary, TextFitDestinationIdentity, TextFitDestinationSummary,
};
pub(crate) use font_freshness::{FontProgramFreshnessError, FontProgramSnapshot};
pub(crate) use native::NativeTextRequestError;
pub(crate) use request::TextFitRequest;
#[cfg(test)]
pub(crate) use request::{
    FinalRtf, MinimumFontScale, RequiredFonts, TextBoxGeometry, TextFitRequestError, TextMargins,
    TextScaleBehavior, TextTransform, TextVerticalAlignment,
};

/// Stable schema of native layout evidence written to build receipts.
pub(crate) const TEXT_FIT_EVIDENCE_SCHEMA: &str = "proflow.text-fit.v3";

/// Version of the Rust/Swift text-fit wire contract.
pub(crate) const TEXT_FIT_PROTOCOL_VERSION: u32 = 5;

#[cfg(test)]
mod tests;
