//! The `proflow` command line: build a plan, then commit it.
//!
//! ```text
//! proflow build  --plan <id|next> [--service <service type>]
//! proflow decide --plan <id|next> --item <item> <decision>
//! proflow background --plan <id|next> --id <background> (--file <image> | --reset)
//! proflow commit --plan <id|next> [--service <service type>]
//! ```
//!
//! `build` reviews the plan with the decisions recorded in its `review.toml`,
//! and renders and verifies it when none is missing. For each open decision it
//! prints the choices it can offer, each as the `decide` command that records
//! it. `commit` installs the verified build. None of them prompts. The exit status is 0 when the command did
//! what it names, 2 when the plan needs decisions first, and 1 on any error.

mod invocation;
pub use invocation::{Command, Invocation, PlanSelector, UsageError, USAGE};
mod display;
#[cfg(test)]
use display::DecideCommand;

use std::fmt;
use std::path::PathBuf;

use crate::project_config::{BackgroundAssetPath, BackgroundId};
use crate::workflow::execute::{
    BuildServiceError, DecisionChoice, EntryOverrideAction, OpenDecision, Remembered,
};
use crate::workflow::report::{BuildEntryAction, BuildServiceResult};
use crate::workstation::{AppliedDefault, RecordedBuild, WorkspaceDiscovery, Workstation};

/// The remembered defaults one build applied and refused.
#[derive(Debug, Default)]
pub struct Defaults {
    /// Recorded as decisions by this build.
    pub applied: Vec<AppliedDefault>,
    /// Not accepted by the plan.
    pub refused: Vec<AppliedDefault>,
}

impl fmt::Display for Defaults {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (heading, defaults) in [
            ("Answered with your default", &self.applied),
            (
                "Your default no longer fits and was not used",
                &self.refused,
            ),
        ] {
            if defaults.is_empty() {
                continue;
            }
            writeln!(formatter, "{heading}:")?;
            for default in defaults {
                writeln!(
                    formatter,
                    "  {}  {}: {}",
                    default.item, default.title, default.choice
                )?;
            }
            writeln!(formatter)?;
        }
        Ok(())
    }
}

/// What a command did.
#[derive(Debug)]
pub enum Outcome {
    /// The plan cannot be built until these decisions are recorded.
    NeedsDecisions {
        /// Plan identity.
        plan_id: String,
        /// Playlist the plan will become.
        playlist_name: String,
        /// Every open decision, in plan order.
        decisions: Vec<OpenDecision>,
        /// File to record them in.
        decisions_file: PathBuf,
        /// Remembered defaults this build applied and refused.
        defaults: Defaults,
    },
    /// The plan is rendered and verified; nothing is installed.
    Built {
        /// Plan identity.
        plan_id: String,
        /// Playlist the plan will become.
        playlist_name: String,
        /// Report of the verified build.
        result: Box<BuildServiceResult>,
        /// Directory holding the build.
        build_directory: PathBuf,
        /// Remembered defaults this build applied and refused.
        defaults: Defaults,
    },
    /// One decision is recorded.
    Decided {
        /// Plan identity.
        plan_id: String,
        /// Item the decision is for.
        item: String,
        /// File it was recorded in.
        decisions_file: PathBuf,
        /// Whether it is now the default for its question.
        as_default: bool,
    },
    /// A background's image for this plan is recorded.
    BackgroundSet {
        /// Plan identity.
        plan_id: String,
        /// Configured background identity.
        id: BackgroundId,
        /// Image now used for it in this plan, or `None` for the configured one.
        file: Option<BackgroundAssetPath>,
        /// File it was recorded in.
        decisions_file: PathBuf,
    },
    /// The verified build is installed.
    Committed {
        /// Plan identity.
        plan_id: String,
        /// Report of the installed build.
        result: Box<BuildServiceResult>,
    },
}

impl Outcome {
    /// Process exit status for this outcome.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::NeedsDecisions { .. } => 2,
            Self::Built { .. }
            | Self::Decided { .. }
            | Self::BackgroundSet { .. }
            | Self::Committed { .. } => 0,
        }
    }

    fn from_build(
        build: RecordedBuild,
        decisions_file: PathBuf,
    ) -> Result<Self, BuildServiceError> {
        let RecordedBuild {
            review,
            open_decisions: decisions,
            applied_defaults: applied,
            refused_defaults: refused,
        } = build;
        let defaults = Defaults { applied, refused };
        let plan_id = review.plan_id().to_string();
        let playlist_name = review.playlist_name().to_string();
        if !decisions.is_empty() {
            return Ok(Self::NeedsDecisions {
                plan_id,
                playlist_name,
                decisions,
                decisions_file,
                defaults,
            });
        }
        let prepared = review.into_prepared()?;
        Ok(Self::Built {
            plan_id,
            playlist_name,
            build_directory: prepared.build_directory().to_path_buf(),
            result: Box::new(prepared.into_result()),
            defaults,
        })
    }
}

/// Run one parsed command against a loaded workstation.
pub async fn run(
    workstation: &Workstation,
    invocation: &Invocation,
) -> Result<Outcome, BuildServiceError> {
    let service = invocation.service.as_deref();
    let plan_id = match &invocation.plan {
        PlanSelector::Id(plan_id) => plan_id.clone(),
        PlanSelector::Next => workstation.next_plan_id(service).await?,
    };
    match &invocation.command {
        Command::Decide {
            item,
            choice,
            as_default,
        } => Ok(Outcome::Decided {
            decisions_file: workstation.decide(&plan_id, item, choice.clone(), *as_default)?,
            item: item.clone(),
            as_default: *as_default,
            plan_id,
        }),
        Command::Background { id, file } => Ok(Outcome::BackgroundSet {
            decisions_file: workstation.use_background_file(&plan_id, id.clone(), file.clone())?,
            id: id.clone(),
            file: file.clone(),
            plan_id,
        }),
        Command::Build => Outcome::from_build(
            workstation.build_recorded(&plan_id, service).await?,
            workstation.executor().review_decisions_path(&plan_id)?,
        ),
        Command::Commit => Ok(Outcome::Committed {
            result: Box::new(workstation.commit_recorded(&plan_id, service, None).await?),
            plan_id,
        }),
    }
}

#[cfg(test)]
mod tests;
