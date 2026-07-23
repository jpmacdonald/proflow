//! Checked Planning Center future-plan lookup window.

/// Number of future days included when discovering a Planning Center plan.
///
/// The checked type keeps config, MCP, workflow, and API callers on one
/// contract. Callers cannot silently clamp or bypass the supported range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlanLookaheadDays(i64);

impl PlanLookaheadDays {
    /// Smallest supported lookup window.
    pub const MIN: i64 = 1;
    /// Largest supported lookup window.
    pub const MAX: i64 = 365;
    /// Default lookup window when no operator or project value is supplied.
    pub const DEFAULT: Self = Self(30);

    /// Create a checked future-plan lookup window.
    pub const fn new(value: i64) -> Result<Self, PlanLookaheadDaysError> {
        if value >= Self::MIN && value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(PlanLookaheadDaysError { value })
        }
    }

    /// Return the number of future days represented by this value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Default for PlanLookaheadDays {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl std::fmt::Display for PlanLookaheadDays {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// An integer outside the supported future-plan lookup window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "days_ahead must be between {} and {}, got {value}",
    PlanLookaheadDays::MIN,
    PlanLookaheadDays::MAX
)]
pub struct PlanLookaheadDaysError {
    value: i64,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn accepts_default_and_range_edges() {
        assert_eq!(PlanLookaheadDays::DEFAULT.get(), 30);
        assert_eq!(
            PlanLookaheadDays::new(PlanLookaheadDays::MIN)
                .expect("minimum should be valid")
                .get(),
            1
        );
        assert_eq!(
            PlanLookaheadDays::new(PlanLookaheadDays::MAX)
                .expect("maximum should be valid")
                .get(),
            365
        );
    }

    #[test]
    fn rejects_values_outside_the_contract() {
        assert!(PlanLookaheadDays::new(0).is_err());
        assert!(PlanLookaheadDays::new(366).is_err());
    }
}
