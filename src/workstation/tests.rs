#![allow(clippy::expect_used)]

use chrono::{DateTime, Duration, Utc};

use super::*;

fn at(hours: i64) -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH + Duration::hours(hours)
}

#[test]
fn the_next_plan_is_the_soonest_of_the_wanted_service_type() {
    let plans = [
        ("late", "Sunday Morning", at(48)),
        ("soon", "Sunday Morning", at(24)),
        ("sooner", "Wednesday Night", at(12)),
    ];
    assert_eq!(soonest_plan(plans, None, 30).expect("soonest"), "sooner");
    assert_eq!(
        soonest_plan(plans, Some("Sunday Morning"), 30).expect("soonest of type"),
        "soon"
    );
}

#[test]
fn no_plan_and_two_plans_at_the_same_time_are_refused() {
    assert!(matches!(
        soonest_plan([("a", "Sunday Morning", at(1))], Some("Other"), 30),
        Err(RequestError::NoUpcomingPlan { days_ahead: 30 })
    ));
    let simultaneous = [
        ("a", "Traditional", at(24)),
        ("b", "Contemporary", at(24)),
        ("c", "Traditional", at(48)),
    ];
    assert!(matches!(
        soonest_plan(simultaneous, None, 30),
        Err(RequestError::AmbiguousNextPlan { candidates })
            if candidates == ["a (Traditional)", "b (Contemporary)"]
    ));
    assert_eq!(
        soonest_plan(simultaneous, Some("Contemporary"), 30).expect("named type"),
        "b"
    );
}
