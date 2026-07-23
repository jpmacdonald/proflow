//! Canonical Planning Center plan identity and operator-facing playlist names.

use chrono::{DateTime, Utc};

use super::types::{Plan, Service};
use super::PlanLookaheadDays;

/// Authoritative plan metadata used by every build transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlanIdentity {
    pub plan_id: String,
    pub service_id: String,
    pub service_name: String,
    pub plan_title: String,
    pub date: DateTime<Utc>,
    pub default_playlist_name: String,
}

/// Failure to bind a caller request to authoritative Planning Center metadata.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanIdentityError {
    /// The requested plan was outside the fetched lookup window.
    #[error("plan '{plan_id}' was not found in the next {days_ahead} days")]
    NotFound {
        plan_id: String,
        days_ahead: PlanLookaheadDays,
    },
}

/// Resolve authoritative plan metadata and the one canonical default playlist name.
pub fn resolve_plan_identity(
    services: &[Service],
    plans: &[Plan],
    plan_id: &str,
    days_ahead: PlanLookaheadDays,
) -> Result<ResolvedPlanIdentity, PlanIdentityError> {
    let plan = plans
        .iter()
        .find(|plan| plan.id == plan_id)
        .ok_or_else(|| PlanIdentityError::NotFound {
            plan_id: plan_id.to_string(),
            days_ahead,
        })?;
    let service_name = services
        .iter()
        .find(|service| service.id == plan.service_id)
        .map_or_else(|| plan.service_name.clone(), |service| service.name.clone());

    Ok(ResolvedPlanIdentity {
        plan_id: plan.id.clone(),
        service_id: plan.service_id.clone(),
        default_playlist_name: format!(
            "{} - {}",
            plan.date.format("%B %-d, %Y"),
            canonical_service_label(&service_name)
        ),
        service_name,
        plan_title: plan.title.clone(),
        date: plan.date,
    })
}

fn canonical_service_label(service_name: &str) -> String {
    let Some((time, description)) = service_name.split_once(char::is_whitespace) else {
        return service_name.to_string();
    };
    let Some((clock, period)) = time
        .strip_suffix("am")
        .map(|clock| (clock, "am"))
        .or_else(|| time.strip_suffix("pm").map(|clock| (clock, "pm")))
    else {
        return service_name.to_string();
    };
    let Some((hour, minute)) = clock.split_once(':') else {
        return service_name.to_string();
    };
    let Ok(hour) = hour.parse::<u8>() else {
        return service_name.to_string();
    };
    if !(1..=12).contains(&hour)
        || minute.len() != 2
        || !minute.bytes().all(|byte| byte.is_ascii_digit())
    {
        return service_name.to_string();
    }
    let compact_time = if minute == "00" {
        format!("{hour}{period}")
    } else {
        format!("{hour}{minute}{period}")
    };
    let description = description.trim();
    if description.is_empty() {
        return compact_time;
    }
    let mut characters = description.chars();
    let Some(first) = characters.next() else {
        return compact_time;
    };
    format!(
        "{compact_time} {}{}",
        first.to_uppercase(),
        characters.as_str()
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use chrono::{TimeZone, Utc};

    use super::*;

    fn plan(service_name: &str) -> Plan {
        Plan {
            id: "plan-1".to_string(),
            service_id: "service-1".to_string(),
            service_name: service_name.to_string(),
            date: Utc
                .with_ymd_and_hms(2026, 7, 19, 14, 0, 0)
                .single()
                .expect("valid date"),
            title: "July 19".to_string(),
            items: Vec::new(),
        }
    }

    #[test]
    fn canonical_playlist_name_uses_authoritative_service_catalog() {
        let services = [Service {
            id: "service-1".to_string(),
            name: "9:00am contemporary".to_string(),
        }];
        let identity = resolve_plan_identity(
            &services,
            &[plan("stale embedded name")],
            "plan-1",
            PlanLookaheadDays::new(60).expect("valid lookahead"),
        )
        .expect("catalog identity");

        assert_eq!(identity.service_name, "9:00am contemporary");
        assert_eq!(
            identity.default_playlist_name,
            "July 19, 2026 - 9am Contemporary"
        );
    }

    #[test]
    fn canonical_service_labels_match_operator_convention() {
        assert_eq!(
            canonical_service_label("9:00am contemporary"),
            "9am Contemporary"
        );
        assert_eq!(
            canonical_service_label("10:30am traditional"),
            "1030am Traditional"
        );
        assert_eq!(canonical_service_label("Sunday Morning"), "Sunday Morning");
    }
}
