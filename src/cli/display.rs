//! Operator outcomes and command-line formatting.

use super::{
    BackgroundId, BuildEntryAction, BuildServiceResult, DecisionChoice, EntryOverrideAction,
    OpenDecision, Outcome, Remembered,
};
use std::fmt;

/// The command line that records one choice.
pub(super) struct DecideCommand<'a> {
    pub(super) plan_id: &'a str,
    pub(super) item: &'a str,
    pub(super) choice: &'a DecisionChoice,
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

pub(super) fn option(
    formatter: &mut fmt::Formatter<'_>,
    name: &str,
    value: Option<&str>,
) -> fmt::Result {
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

/// What a build did, counted by what happened to each item.
struct Counts<'a>(&'a BuildServiceResult);

impl fmt::Display for Counts<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = |wanted: fn(&BuildEntryAction) -> bool| {
            self.0
                .entries
                .iter()
                .filter(|entry| wanted(&entry.action))
                .count()
        };
        let parts = [
            (
                count(|action| matches!(action, BuildEntryAction::Generated)),
                "generated",
            ),
            (
                count(|action| matches!(action, BuildEntryAction::Edited)),
                "edited",
            ),
            (
                count(|action| matches!(action, BuildEntryAction::Restyled)),
                "restyled",
            ),
            (
                count(|action| matches!(action, BuildEntryAction::Library)),
                "used unchanged",
            ),
        ];
        write!(formatter, "{} in the playlist", self.0.total_items)?;
        let described = parts
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, what)| format!("{count} {what}"))
            .collect::<Vec<_>>();
        if !described.is_empty() {
            write!(formatter, " ({})", described.join(", "))?;
        }
        write!(formatter, ", {} skipped", self.0.skipped_count)
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

/// List every open decision with the command for each of its choices.
fn write_open_decisions(
    formatter: &mut fmt::Formatter<'_>,
    plan_id: &str,
    playlist_name: &str,
    decisions: &[OpenDecision],
    decisions_file: &std::path::Path,
) -> fmt::Result {
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
                write_open_decisions(formatter, plan_id, playlist_name, decisions, decisions_file)
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
            Self::BackgroundSet {
                plan_id,
                id,
                file,
                decisions_file,
            } => {
                match file {
                    Some(file) => writeln!(
                        formatter,
                        "Plan {plan_id} uses {} for background '{id}'.",
                        file.as_path().display()
                    )?,
                    None => writeln!(
                        formatter,
                        "Plan {plan_id} uses the configured image for background '{id}'."
                    )?,
                }
                writeln!(formatter, "Recorded in {}.", decisions_file.display())
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
