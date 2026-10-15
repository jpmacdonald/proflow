//! Checked command-line arguments.

use super::{
    BackgroundAssetPath, BackgroundId, DecisionChoice, EntryOverrideAction, WorkspaceDiscovery,
};

/// How to call the command.
pub const USAGE: &str = "\
usage: proflow build  --plan <id|next> [--service <service type>]
       proflow decide --plan <id|next> --item <item> <decision>
       proflow background --plan <id|next> --id <background> (--file <image> | --reset)
       proflow commit --plan <id|next> [--service <service type>]

  build    review the plan with the decisions in its review.toml, then render
           and verify it; exits 2 and lists the open decisions if any remain
  decide   record one decision in the plan's review.toml
  background
           use an image for a configured background in this plan only;
           <image> is a path below the data root. --reset goes back to the
           configured image
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
    /// Use an image for a configured background in this plan only.
    Background {
        /// Configured background identity.
        id: BackgroundId,
        /// Image below the data root, or `None` for the configured image.
        file: Option<BackgroundAssetPath>,
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
    #[error("expected `build`, `decide`, `background` or `commit`, found '{found}'")]
    UnknownCommand {
        /// What was given.
        found: String,
    },
    /// No command was given.
    #[error("expected `build`, `decide`, `background` or `commit`")]
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
            Some(command)
                if matches!(
                    command.as_str(),
                    "build" | "decide" | "background" | "commit"
                ) =>
            {
                command
            }
            Some(found) => return Err(UsageError::UnknownCommand { found }),
            None => return Err(UsageError::MissingCommand),
        };
        let deciding = command == "decide";
        let background = command == "background";
        let mut background_id = None;
        let mut background_file = None;
        let mut reset = None;
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
                "--id" if background => set_once(&mut background_id, "--id", value())?,
                "--file" if background => set_once(&mut background_file, "--file", value())?,
                "--reset" if background => set_once(&mut reset, "--reset", Some(()))?,
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
                "background" => {
                    let problem = |problem: String| UsageError::Decision { problem };
                    Command::Background {
                        id: BackgroundId::new(background_id.ok_or_else(|| {
                            problem("background requires --id <background>".to_string())
                        })?)
                        .map_err(problem)?,
                        file: match (background_file, reset) {
                            (Some(file), None) => {
                                Some(BackgroundAssetPath::new(file).map_err(problem)?)
                            }
                            (None, Some(())) => None,
                            _ => {
                                return Err(problem(
                                    "background requires exactly one of --file <image> or --reset"
                                        .to_string(),
                                ));
                            }
                        },
                    }
                }
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
