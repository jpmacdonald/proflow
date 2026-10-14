//! Discover and review the selected service plan.

use super::{
    BuildRequest, BuildReview, BuildServiceError, PlanningCenterClient, RequestError,
    ServiceBuildExecutor,
};

/// The Planning Center boundary around a synchronous review: compute the
/// lookup, perform it, hand the snapshot to the executor.
pub async fn review_service_request(
    planning_center: &PlanningCenterClient,
    executor: &ServiceBuildExecutor<'_>,
    request: BuildRequest,
) -> Result<BuildReview, BuildServiceError> {
    let snapshot = planning_center
        .capture_plan(executor.plan_lookup(&request)?)
        .await?;
    executor.review_snapshot(request, snapshot)
}

/// Choose the soonest plan from `(id, service type, date)` triples.
///
/// Two service types often meet at the same time. Rather than guess between
/// them, the choice is refused and names the candidates.
pub(super) fn soonest_plan<'a>(
    plans: impl IntoIterator<Item = (&'a str, &'a str, chrono::DateTime<chrono::Utc>)>,
    service_name: Option<&str>,
    days_ahead: i64,
) -> Result<&'a str, RequestError> {
    let mut candidates = plans
        .into_iter()
        .filter(|(_, service, _)| service_name.is_none_or(|wanted| wanted == *service))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, _, date)| *date);
    let Some((first, _, soonest)) = candidates.first().copied() else {
        return Err(RequestError::NoUpcomingPlan { days_ahead });
    };
    candidates.retain(|(_, _, date)| *date == soonest);
    if candidates.len() > 1 {
        return Err(RequestError::AmbiguousNextPlan {
            candidates: candidates
                .iter()
                .map(|(id, service, _)| format!("{id} ({service})"))
                .collect(),
        });
    }
    Ok(first)
}
