//! What the operator chose for a question before, and what they said to
//! choose for it from now on.
//!
//! A question is an item title and a [`DecisionKind`]: "Amazing Grace, which
//! file?" is the same question next month, whatever plan it is asked for.
//! The memory lives in `<data root>/decision-memory.toml`:
//!
//! ```toml
//! [[question]]
//! title = "amazing grace"
//! kind = "which_file"
//!
//! [question.default.action]          # optional: apply this without asking
//! kind = "use_existing"
//! file_path = "/Library/Amazing Grace.pro"
//!
//! [[question.answer]]
//! times = 3
//! last = "2026-10-05T19:30:00Z"
//!
//! [question.answer.choice.action]
//! kind = "use_existing"
//! file_path = "/Library/Amazing Grace.pro"
//! ```
//!
//! Remembered answers only order and annotate the choices a build offers. A
//! build answers a question by itself only when that question has a
//! `default`, and it says so when it does.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::execute::{DecisionChoice, DecisionKind, OpenDecision, Remembered};

const MEMORY_FILE: &str = "decision-memory.toml";

/// Every remembered question of one data root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionMemory {
    #[serde(default, rename = "question", skip_serializing_if = "Vec::is_empty")]
    questions: Vec<RememberedQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberedQuestion {
    /// Item title, lower-cased with single spaces.
    title: String,
    kind: DecisionKind,
    /// The choice a build makes for this question without asking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<DecisionChoice>,
    #[serde(default, rename = "answer", skip_serializing_if = "Vec::is_empty")]
    answers: Vec<RememberedAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberedAnswer {
    choice: DecisionChoice,
    times: u32,
    last: DateTime<Utc>,
}

/// A decision-memory file could not be read, parsed or written.
#[derive(Debug, thiserror::Error)]
#[error("'{}' {problem}", path.display())]
pub struct DecisionMemoryError {
    path: PathBuf,
    problem: String,
}

impl DecisionMemory {
    /// Where a data root keeps its decision memory.
    #[must_use]
    pub fn path(project_data_root: &Path) -> PathBuf {
        project_data_root.join(MEMORY_FILE)
    }

    /// Read the memory. A data root that has remembered nothing has no file.
    pub fn read(path: &Path) -> Result<Self, DecisionMemoryError> {
        let failed = |problem: String| DecisionMemoryError {
            path: path.to_path_buf(),
            problem,
        };
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).map_err(|error| failed(format!("is invalid: {error}")))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(failed(format!("could not be read: {error}"))),
        }
    }

    /// Replace the memory file, through a temporary file and a rename.
    pub fn write(&self, path: &Path) -> Result<(), DecisionMemoryError> {
        let failed = |error: &dyn std::fmt::Display| DecisionMemoryError {
            path: path.to_path_buf(),
            problem: format!("could not be written: {error}"),
        };
        let text = toml::to_string(self).map_err(|error| failed(&error))?;
        let temporary = path.with_extension("toml.tmp");
        std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, path))
            .map_err(|error| failed(&error))
    }

    /// Remember that `choice` answered the question `title` / `kind` at `at`.
    pub fn remember(
        &mut self,
        title: &str,
        kind: DecisionKind,
        choice: &DecisionChoice,
        at: DateTime<Utc>,
    ) {
        let question = self.question_mut(title, kind);
        match question
            .answers
            .iter_mut()
            .find(|answer| &answer.choice == choice)
        {
            Some(answer) => {
                answer.times = answer.times.saturating_add(1);
                answer.last = at;
            }
            None => question.answers.push(RememberedAnswer {
                choice: choice.clone(),
                times: 1,
                last: at,
            }),
        }
    }

    /// Make `choice` what a build chooses for this question without asking,
    /// or stop choosing for it with `None`.
    pub fn set_default(&mut self, title: &str, kind: DecisionKind, choice: Option<DecisionChoice>) {
        self.question_mut(title, kind).default = choice;
    }

    /// The choice a build makes for `decision` without asking, if one is set.
    #[must_use]
    pub fn default_for(&self, decision: &OpenDecision) -> Option<&DecisionChoice> {
        self.question(&decision.title, decision.kind)?
            .default
            .as_ref()
    }

    /// Annotate the options of `decision` with their history and put the
    /// ones chosen before first, most recent first. Options never chosen keep
    /// the order the review gave them.
    pub fn rank(&self, decision: &mut OpenDecision) {
        let Some(question) = self.question(&decision.title, decision.kind) else {
            return;
        };
        for option in &mut decision.options {
            option.remembered = question
                .answers
                .iter()
                .find(|answer| answer.choice == option.choice)
                .map(|answer| Remembered {
                    times: answer.times,
                    last: answer.last,
                });
        }
        decision.options.sort_by_key(|option| {
            std::cmp::Reverse(option.remembered.map(|remembered| remembered.last))
        });
    }

    fn question(&self, title: &str, kind: DecisionKind) -> Option<&RememberedQuestion> {
        let title = question_title(title);
        self.questions
            .iter()
            .find(|question| question.kind == kind && question.title == title)
    }

    fn question_mut(&mut self, title: &str, kind: DecisionKind) -> &mut RememberedQuestion {
        let title = question_title(title);
        let position = self
            .questions
            .iter()
            .position(|question| question.kind == kind && question.title == title)
            .unwrap_or_else(|| {
                self.questions.push(RememberedQuestion {
                    title,
                    kind,
                    default: None,
                    answers: Vec::new(),
                });
                self.questions.len() - 1
            });
        &mut self.questions[position]
    }
}

/// The questions a plan's latest build left open, kept so that a decision
/// recorded afterwards is known to answer one of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenQuestions {
    #[serde(default, rename = "open", skip_serializing_if = "Vec::is_empty")]
    open: Vec<OpenQuestion>,
}

/// One open question: which item asked it, and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenQuestion {
    /// Output key of the plan item.
    pub item: String,
    /// The item's Planning Center title.
    pub title: String,
    /// What kind of question it is.
    pub kind: DecisionKind,
}

impl OpenQuestions {
    /// The questions `decisions` ask.
    #[must_use]
    pub fn of(decisions: &[OpenDecision]) -> Self {
        Self {
            open: decisions
                .iter()
                .map(|decision| OpenQuestion {
                    item: decision.item.clone(),
                    title: decision.title.clone(),
                    kind: decision.kind,
                })
                .collect(),
        }
    }

    /// The file beside a plan's `review.toml` that holds them.
    #[must_use]
    pub fn path(review_decisions: &Path) -> PathBuf {
        review_decisions.with_file_name("open-questions.toml")
    }

    /// Read a plan's open questions; none if no build has listed any.
    pub fn read(path: &Path) -> Result<Self, DecisionMemoryError> {
        let failed = |problem: String| DecisionMemoryError {
            path: path.to_path_buf(),
            problem,
        };
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).map_err(|error| failed(format!("is invalid: {error}")))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(failed(format!("could not be read: {error}"))),
        }
    }

    /// Replace a plan's open questions.
    pub fn write(&self, path: &Path) -> Result<(), DecisionMemoryError> {
        let failed = |error: &dyn std::fmt::Display| DecisionMemoryError {
            path: path.to_path_buf(),
            problem: format!("could not be written: {error}"),
        };
        let text = toml::to_string(self).map_err(|error| failed(&error))?;
        std::fs::write(path, text).map_err(|error| failed(&error))
    }

    /// The open question `item` asked, if it asked one.
    #[must_use]
    pub fn asked_by(&self, item: &str) -> Option<&OpenQuestion> {
        self.open.iter().find(|question| question.item == item)
    }
}

/// Letter case and spacing do not make a different question.
fn question_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests;
