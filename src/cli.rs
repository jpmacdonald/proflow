//! The `proflow` command line: build a plan, then commit it.
//!
//! ```text
//! proflow build  --plan <id|next> [--service <service type>]
//! proflow commit --plan <id|next> [--service <service type>]
//! ```
//!
//! `build` reviews the plan with the decisions recorded in its `review.toml`,
//! and renders and verifies it when none is missing. `commit` installs that
//! verified build. Neither prompts. The exit status is 0 when the command did
//! what it names, 2 when the plan needs decisions first, and 1 on any error.

use std::fmt;
use std::path::PathBuf;

use crate::workflow::execute::{BuildReview, BuildServiceError, OpenDecision};
use crate::workflow::report::BuildServiceResult;
use crate::workstation::{WorkspaceDiscovery, Workstation};

/// How to call the command.
pub const USAGE: &str = "\
usage: proflow build  --plan <id|next> [--service <service type>]
       proflow commit --plan <id|next> [--service <service type>]

  build    review the plan with the decisions in its review.toml, then render
           and verify it; exits 2 and lists the open decisions if any remain
  commit   install the verified build of the plan as it stands now

  --plan next             the soonest upcoming plan
  --service <name>        the plan's service type; required with `next` when
                          two service types share the soonest time
  --configured-workspace  trust the configured ProPresenter root without
                          consulting the active show (isolated QA only)";

/// Which step to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Review, render and verify.
    Build,
    /// Install the verified build.
    Commit,
}

/// Which plan to run it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanSelector {
    /// One exact Planning Center plan.
    Id(String),
    /// The soonest upcoming plan.
    Next,
}

/// One parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Step to run.
    pub command: Command,
    /// Plan to run it on.
    pub plan: PlanSelector,
    /// Asserted service type, when given.
    pub service: Option<String>,
    /// How the `ProPresenter` workspace is chosen.
    pub workspace: WorkspaceDiscovery,
}

/// A command line that names no runnable command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    /// The first argument is not a command.
    #[error("expected `build` or `commit`, found '{found}'")]
    UnknownCommand {
        /// What was given.
        found: String,
    },
    /// No command was given.
    #[error("expected `build` or `commit`")]
    MissingCommand,
    /// `--plan` was not given.
    #[error("--plan <id|next> is required")]
    MissingPlan,
    /// An option that takes a value was last on the line.
    #[error("{option} requires a value")]
    MissingValue {
        /// Option without its value.
        option: &'static str,
    },
    /// An option was given twice.
    #[error("{option} may be given only once")]
    Repeated {
        /// Repeated option.
        option: &'static str,
    },
    /// An argument that is not an option of this command.
    #[error("unexpected argument '{argument}'")]
    Unexpected {
        /// What was given.
        argument: String,
    },
}

impl Invocation {
    /// Parse the arguments after the program name.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, UsageError> {
        let mut arguments = arguments.into_iter();
        let command = match arguments.next().as_deref() {
            Some("build") => Command::Build,
            Some("commit") => Command::Commit,
            Some(found) => {
                return Err(UsageError::UnknownCommand {
                    found: found.to_string(),
                });
            }
            None => return Err(UsageError::MissingCommand),
        };
        let mut plan = None;
        let mut service = None;
        let mut workspace = None;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--plan" => set_once(&mut plan, "--plan", arguments.next())?,
                "--service" => set_once(&mut service, "--service", arguments.next())?,
                "--configured-workspace" => set_once(
                    &mut workspace,
                    "--configured-workspace",
                    Some(WorkspaceDiscovery::ConfiguredOnly),
                )?,
                _ => return Err(UsageError::Unexpected { argument }),
            }
        }
        Ok(Self {
            command,
            plan: match plan.ok_or(UsageError::MissingPlan)? {
                plan if plan == "next" => PlanSelector::Next,
                plan => PlanSelector::Id(plan),
            },
            service,
            workspace: workspace.unwrap_or(WorkspaceDiscovery::ActiveShow),
        })
    }
}

fn set_once<T>(
    slot: &mut Option<T>,
    option: &'static str,
    value: Option<T>,
) -> Result<(), UsageError> {
    if slot.is_some() {
        return Err(UsageError::Repeated { option });
    }
    *slot = Some(value.ok_or(UsageError::MissingValue { option })?);
    Ok(())
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
            Self::Built { .. } | Self::Committed { .. } => 0,
        }
    }

    fn from_review(
        review: BuildReview,
        decisions_file: PathBuf,
    ) -> Result<Self, BuildServiceError> {
        let plan_id = review.plan_id().to_string();
        let playlist_name = review.playlist_name().to_string();
        let decisions = review.open_decisions();
        if !decisions.is_empty() {
            return Ok(Self::NeedsDecisions {
                plan_id,
                playlist_name,
                decisions,
                decisions_file,
            });
        }
        let prepared = review.into_prepared()?;
        Ok(Self::Built {
            plan_id,
            playlist_name,
            build_directory: prepared.build_directory().to_path_buf(),
            result: Box::new(prepared.into_result()),
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
    match invocation.command {
        Command::Build => Outcome::from_review(
            workstation.build_recorded(&plan_id, service).await?,
            workstation.executor().review_decisions_path(&plan_id)?,
        ),
        Command::Commit => Ok(Outcome::Committed {
            result: Box::new(workstation.commit_recorded(&plan_id, service, None).await?),
            plan_id,
        }),
    }
}

struct Counts<'a>(&'a BuildServiceResult);

impl fmt::Display for Counts<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let result = self.0;
        write!(
            formatter,
            "{} in the playlist ({} generated, {} from the library), {} skipped",
            result.total_items, result.generated_count, result.library_count, result.skipped_count
        )
    }
}

fn write_warnings(formatter: &mut fmt::Formatter<'_>, warnings: &[String]) -> fmt::Result {
    if warnings.is_empty() {
        return Ok(());
    }
    writeln!(formatter, "\nWarnings:")?;
    warnings
        .iter()
        .try_for_each(|warning| writeln!(formatter, "  - {warning}"))
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedsDecisions {
                plan_id,
                playlist_name,
                decisions,
                decisions_file,
            } => {
                writeln!(
                    formatter,
                    "Plan {plan_id} \"{playlist_name}\" needs {} decision(s) before it can be built.\n",
                    decisions.len()
                )?;
                for decision in decisions {
                    writeln!(formatter, "  {}  {}", decision.item, decision.title)?;
                    writeln!(formatter, "      {}", decision.question)?;
                }
                writeln!(
                    formatter,
                    "\nRecord each one in {} and build again. To leave an item out:\n",
                    decisions_file.display()
                )?;
                if let Some(first) = decisions.first() {
                    writeln!(formatter, "  [[decision]]")?;
                    writeln!(formatter, "  item = \"{}\"", first.item)?;
                    writeln!(formatter, "  skip = true\n")?;
                }
                writeln!(
                    formatter,
                    "Every other kind of decision is described in docs/review-decisions.md."
                )
            }
            Self::Built {
                plan_id,
                playlist_name,
                result,
                build_directory,
            } => {
                writeln!(
                    formatter,
                    "Plan {plan_id} \"{playlist_name}\" is built and verified: {}.",
                    Counts(result)
                )?;
                writeln!(formatter, "Nothing is installed yet. To install it:\n")?;
                writeln!(formatter, "  proflow commit --plan {plan_id}\n")?;
                writeln!(formatter, "Build: {}", build_directory.display())?;
                write_warnings(formatter, &result.warnings)
            }
            Self::Committed { plan_id, result } => {
                writeln!(
                    formatter,
                    "Plan {plan_id} is installed: {}.",
                    Counts(result)
                )?;
                writeln!(formatter, "Playlist: {}", result.playlist_path.display())?;
                writeln!(formatter, "Receipt:  {}", result.receipt_path.display())?;
                write_warnings(formatter, &result.warnings)
            }
        }
    }
}

#[cfg(test)]
mod tests;
