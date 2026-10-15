//! MCP request and response translations. These functions perform no IO.

use super::super::schema::{
    format_category, ExplainRuleMatchArgs, ItemResponse, ReviewedMediaAssetResponse,
    ReviewedPreviewResponse, ScriptureResponse, SongResponse,
};
use crate::workflow::classify;
use crate::workstation::RecordedBuild;

pub(super) fn explanation_item(args: &ExplainRuleMatchArgs) -> crate::planning_center::types::Item {
    let category = args
        .category
        .unwrap_or(crate::planning_center::types::Category::Text);
    let item = crate::planning_center::types::Item {
        id: "__explain__".to_string(),
        position: 1,
        title: args.title.clone(),
        description: args.description.clone(),
        category,
        note: None,
        song: args
            .song_title
            .as_ref()
            .map(|title| crate::planning_center::types::Song {
                title: title.clone(),
                author: None,
                copyright: None,
                ccli: None,
                themes: None,
                lyrics: None,
                arrangement: None,
            }),
        scripture: args.scripture_reference.as_ref().map(|reference| {
            crate::planning_center::types::Scripture {
                reference: reference.clone(),
                text: None,
                translation: None,
            }
        }),
    };

    item
}

pub(super) fn item_response(item: &crate::planning_center::types::Item) -> ItemResponse {
    ItemResponse {
        id: item.id.clone(),
        position: item.position,
        title: item.title.clone(),
        description: item.description.clone(),
        category: format_category(item.category).to_string(),
        note: item.note.clone(),
        song: item.song.as_ref().map(|s| SongResponse {
            title: s.title.clone(),
            author: s.author.clone(),
            lyrics: s.lyrics.clone(),
        }),
        scripture: item.scripture.as_ref().map(|s| ScriptureResponse {
            reference: s.reference.clone(),
            text: s.text.clone(),
        }),
    }
}

pub(super) fn reviewed_preview(recorded: RecordedBuild) -> ReviewedPreviewResponse {
    let RecordedBuild {
        review: reviewed,
        open_decisions,
        applied_defaults,
        refused_defaults,
    } = recorded;
    let plan_title = reviewed.plan_title().to_string();
    let service_name = reviewed.service_name().to_string();
    let date = reviewed.date().format("%Y-%m-%d").to_string();
    let entries = classify::render_preview(reviewed.plans());
    let playlist_name = reviewed.playlist_name().to_string();
    let package_mode = reviewed.playlist_export_mode();
    let media_assets = reviewed
        .additional_media_assets()
        .iter()
        .map(|asset| ReviewedMediaAssetResponse {
            path: asset.source_path.display().to_string(),
            archive_path: asset.archive_path.clone(),
        })
        .collect();
    let summary = classify::PreviewSummary::from_entries(&entries);
    let materialized = reviewed.materialized_result().cloned();
    let result = classify::PreviewResult {
        plan_title,
        service_name,
        date,
        entries,
        summary,
    };

    let preview_revision = match reviewed {
        crate::workflow::execute::BuildReview::Prepared(prepared) => Some(prepared.revision()),
        crate::workflow::execute::BuildReview::NeedsReview(_) => None,
    };

    ReviewedPreviewResponse {
        preview_revision,
        open_decisions,
        applied_defaults,
        refused_defaults,
        playlist_name,
        package_mode,
        media_assets,
        materialized,
        preview: result,
    }
}
