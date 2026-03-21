//! Arrangement selection for `ProPresenter` presentations.
//!
//! Sets the active arrangement on a presentation by finding the arrangement
//! with the matching name and setting `selected_arrangement` to its UUID.

use super::generated::rv_data;

/// Set the active arrangement on a presentation by name.
///
/// Searches through `presentation.arrangements` for one whose `name` matches
/// (case-insensitive), then sets `selected_arrangement` to that UUID.
/// Returns `true` if found and set, `false` if no matching arrangement exists.
pub fn select_arrangement_by_name(
    presentation: &mut rv_data::Presentation,
    name: &str,
) -> bool {
    let target = name.to_lowercase();
    let uuid = presentation
        .arrangements
        .iter()
        .find(|a| a.name.to_lowercase() == target)
        .and_then(|a| a.uuid.clone());

    if let Some(uuid) = uuid {
        presentation.selected_arrangement = Some(uuid);
        true
    } else {
        false
    }
}
