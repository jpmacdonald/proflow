//! The `proflow` command line: build a plan, then commit it.
//!
//! ```text
//! proflow build  --plan <id|next> [--service <service type>]
//! proflow decide --plan <id|next> --item <item> <decision>
//! proflow commit --plan <id|next> [--service <service type>]
//! ```
//!
//! `build` reviews the plan with the decisions recorded in its `review.toml`,
//! and renders and verifies it when none is missing. For each open decision it
//! prints the choices it can offer, each as the `decide` command that records
//! it. `commit` installs the verified build. None of them prompts. The exit status is 0 when the command did
//! what it names, 2 when the plan needs decisions first, and 1 on any error.

use std::fmt;
use std::path::PathBuf;

use crate::project_config::BackgroundId;
use crate::workflow::execute::{
    BuildServiceError, DecisionChoice, EntryOverrideAction, OpenDecision, Remembered,
};
use crate::workflow::report::BuildServiceResult;
use crate::workstation::{AppliedDefault, RecordedBuild, WorkspaceDiscovery, Workstation};

/// How to call the command.
pub const USAGE: &str = "\
usage: proflow build  --plan <id|next> [--service <service type>]
       proflow decide --plan <id|next> --item <item> <decision>
       proflow commit --plan <id|next> [--service <service type>]

  build    review the plan with the decisions in its review.toml, then render
           and verify it; exits 2 and lists the open decisions if any remain
  decide   record one decision in the plan's review.toml
  commit   install the verified build of the plan as it stands now

  <decision> is one of:
    --skip                               leave the item out
    --use <file.pro> [--arrangement <name>]
                                         use this library presentation
    --arrangement <name>                 keep the proposed presentation,
                                         select this arrangement
    --generate [--background <id>]       render the item as a new presentation
    --edit <file.pro> [--background <id>]
                                         render the description into this file
    --background <id>                    keep the proposed render, change its
                                         background
  --default   also choose this for the same question in later plans, without
              being asked; the build reports each default it applies

  --plan next             the soonest upcoming plan
  --service <name>        the plan's service type; required with `next` when
                          two service types share the soonest time
  --configured-workspace  trust the configured ProPresenter root without
                          consulting the active show (isolated QA only)";

/// Which step to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Review, render and verify.
    Build,
    /// Record one decision.
    Decide {
        /// Output key of the plan item.
        item: String,
        /// What is decided for it.
        choice: DecisionChoice,
        /// Also choose this for the same question from now on.
        as_default: bool,
    },
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
    #[error("expected `build`, `decide` or `commit`, found '{found}'")]
    UnknownCommand {
        /// What was given.
        found: String,
    },
    /// No command was given.
    #[error("expected `build`, `decide` or `commit`")]
    MissingCommand,
    /// `decide` was given without `--item`.
    #[error("decide requires --item <item>")]
    MissingItem,
    /// The decision options do not name exactly one decision.
    #[error("{problem}")]
    Decision {
        /// What is wrong with them.
        problem: String,
    },
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
        let command = match arguments.next() {
            Some(command) if matches!(command.as_str(), "build" | "decide" | "commit") => command,
            Some(found) => return Err(UsageError::UnknownCommand { found }),
            None => return Err(UsageError::MissingCommand),
        };
        let deciding = command == "decide";
        let mut plan = None;
        let mut service = None;
        let mut workspace = None;
        let mut item = None;
        let mut decision = DecisionFlags::default();
        let mut as_default = None;
        while let Some(argument) = arguments.next() {
            let mut value = || arguments.next();
            match argument.as_str() {
                "--plan" => set_once(&mut plan, "--plan", value())?,
                "--service" => set_once(&mut service, "--service", value())?,
                "--configured-workspace" => set_once(
                    &mut workspace,
                    "--configured-workspace",
                    Some(WorkspaceDiscovery::ConfiguredOnly),
                )?,
                "--item" if deciding => set_once(&mut item, "--item", value())?,
                "--default" if deciding => set_once(&mut as_default, "--default", Some(()))?,
                "--skip" if deciding => set_once(&mut decision.skip, "--skip", Some(()))?,
                "--generate" if deciding => {
                    set_once(&mut decision.generate, "--generate", Some(()))?;
                }
                "--use" if deciding => set_once(&mut decision.use_file, "--use", value())?,
                "--edit" if deciding => set_once(&mut decision.edit_file, "--edit", value())?,
                "--arrangement" if deciding => {
                    set_once(&mut decision.arrangement, "--arrangement", value())?;
                }
                "--background" if deciding => {
                    set_once(&mut decision.background, "--background", value())?;
                }
                _ => return Err(UsageError::Unexpected { argument }),
            }
        }
        Ok(Self {
            command: match command.as_str() {
                "build" => Command::Build,
                "commit" => Command::Commit,
                _ => Command::Decide {
                    item: item.ok_or(UsageError::MissingItem)?,
                    choice: decision.into_choice()?,
                    as_default: as_default.is_some(),
                },
            },
            plan: match plan.ok_or(UsageError::MissingPlan)? {
                plan if plan == "next" => PlanSelector::Next,
                plan => PlanSelector::Id(plan),
            },
            service,
            workspace: workspace.unwrap_or(WorkspaceDiscovery::ActiveShow),
        })
    }
}

/// The decision options of one `decide` command line.
#[derive(Default)]
struct DecisionFlags {
    skip: Option<()>,
    generate: Option<()>,
    use_file: Option<String>,
    edit_file: Option<String>,
    arrangement: Option<String>,
    background: Option<String>,
}

impl DecisionFlags {
    /// The one decision these options name.
    fn into_choice(self) -> Result<DecisionChoice, UsageError> {
        let problem = |problem: &str| UsageError::Decision {
            problem: problem.to_string(),
        };
        let background = self
            .background
            .map(BackgroundId::new)
            .transpose()
            .map_err(|error| problem(&error))?;
        let action = match (
            self.skip,
            self.use_file,
            self.edit_file,
            self.generate,
            self.arrangement,
            background,
        ) {
            (Some(()), None, None, None, None, None) => return Ok(DecisionChoice::Skip),
            (None, Some(file_path), None, None, arrangement, None) => {
                EntryOverrideAction::UseExisting {
                    file_path,
                    arrangement,
                }
            }
            (None, None, Some(file_path), None, None, background) => {
                EntryOverrideAction::EditDescription {
                    file_path,
                    background,
                }
            }
            (None, None, None, Some(()), None, background) => {
                EntryOverrideAction::GenerateNew { background }
            }
            (None, None, None, None, Some(arrangement), None) => {
                EntryOverrideAction::SelectArrangement { arrangement }
            }
            (None, None, None, None, None, Some(background)) => {
                EntryOverrideAction::SetBackground { background }
            }
            (None, None, None, None, None, None) => {
                return Err(problem(
                    "decide requires a decision: --skip, --use, --arrangement, --generate, --edit or --background",
                ));
            }
            _ => {
                return Err(problem(
                    "these options do not name one decision; see the usage below",
                ));
            }
        };
        Ok(DecisionChoice::Action(action))
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
            Self::Built { .. } | Self::Decided { .. } | Self::Committed { .. } => 0,
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

/// The command line that records one choice.
struct DecideCommand<'a> {
    plan_id: &'a str,
    item: &'a str,
    choice: &'a DecisionChoice,
}

impl fmt::Display for DecideCommand<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "proflow decide --plan {} --item {}",
            quoted(self.plan_id),
            quoted(self.item)
        )?;
        match self.choice {
            DecisionChoice::Skip => write!(formatter, " --skip"),
            DecisionChoice::Action(EntryOverrideAction::UseExisting {
                file_path,
                arrangement,
            }) => {
                option(formatter, "--use", Some(file_path))?;
                option(formatter, "--arrangement", arrangement.as_deref())
            }
            DecisionChoice::Action(EntryOverrideAction::EditDescription {
                file_path,
                background,
            }) => {
                option(formatter, "--edit", Some(file_path))?;
                option(
                    formatter,
                    "--background",
                    background.as_ref().map(BackgroundId::as_str),
                )
            }
            DecisionChoice::Action(EntryOverrideAction::GenerateNew { background }) => {
                write!(formatter, " --generate")?;
                option(
                    formatter,
                    "--background",
                    background.as_ref().map(BackgroundId::as_str),
                )
            }
            DecisionChoice::Action(EntryOverrideAction::SelectArrangement { arrangement }) => {
                option(formatter, "--arrangement", Some(arrangement))
            }
            DecisionChoice::Action(EntryOverrideAction::SetBackground { background }) => {
                option(formatter, "--background", Some(background.as_str()))
            }
        }
    }
}

fn option(formatter: &mut fmt::Formatter<'_>, name: &str, value: Option<&str>) -> fmt::Result {
    value.map_or(Ok(()), |value| {
        write!(formatter, " {name} {}", quoted(value))
    })
}

/// `value` as one shell word.
fn quoted(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:@+,".contains(&byte));
    if plain {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
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
                defaults,
            } => {
                write!(formatter, "{defaults}")?;
                writeln!(
                    formatter,
                    "Plan {plan_id} \"{playlist_name}\" needs {} decision(s) before it can be built.\n",
                    decisions.len()
                )?;
                for decision in decisions {
                    writeln!(formatter, "  {}  {}", decision.item, decision.title)?;
                    writeln!(formatter, "      {}", decision.question)?;
                    for option in &decision.options {
                        match option.remembered {
                            Some(Remembered { times, last }) => writeln!(
                                formatter,
                                "      - {} (chosen {times}x, last {}):",
                                option.summary,
                                last.format("%Y-%m-%d")
                            )?,
                            None => writeln!(formatter, "      - {}:", option.summary)?,
                        }
                        writeln!(
                            formatter,
                            "          {}",
                            DecideCommand {
                                plan_id,
                                item: &decision.item,
                                choice: &option.choice
                            }
                        )?;
                    }
                    writeln!(formatter)?;
                }
                writeln!(
                    formatter,
                    "Run one command for each item, then build again. Add --default to a command to\nhave that choice made for the same question in later plans. `proflow decide`\ntakes other decisions too (see docs/review-decisions.md); all of them are\nrecorded in {}",
                    decisions_file.display()
                )
            }
            Self::Decided {
                plan_id,
                item,
                decisions_file,
                as_default,
            } => {
                writeln!(
                    formatter,
                    "Recorded the decision for {item} in {}.",
                    decisions_file.display()
                )?;
                if *as_default {
                    writeln!(
                        formatter,
                        "It is now the default for this question in later plans."
                    )?;
                }
                writeln!(formatter, "When every item is decided:\n")?;
                writeln!(formatter, "  proflow build --plan {plan_id}")
            }
            Self::Built {
                plan_id,
                playlist_name,
                result,
                build_directory,
                defaults,
            } => {
                write!(formatter, "{defaults}")?;
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
