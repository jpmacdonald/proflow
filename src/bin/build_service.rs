//! Build a `ProPresenter` service playlist from a Planning Center plan.
//!
//! Usage:
//! ```text
//! cargo run --bin build_service -- <plan_id> <service_name> [playlist_name] [--skip <output_key> ...] [--decisions decisions.json] [--library-local]
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use proflow::bible::BibleService;
use proflow::config::Config;
use proflow::paths::{expand_user_path, BuildLocations, PROJECT_CONFIG_FILE};
use proflow::planning_center::PlanningCenterClient;
#[cfg(test)]
use proflow::project_config::RawProjectConfig;
use proflow::project_config::{load_project_config, BackgroundId, ProjectConfig};
use proflow::propresenter::library::LibraryCatalog;
use proflow::propresenter::playlist::{PlaylistExportIntent, PlaylistExportMode, PlaylistMetadata};
use proflow::workflow::execute::{
    BuildRequest, EntryOverride, OverrideAction, OverrideSlideType, RenderAssetSnapshot,
    ServiceBuildExecutor,
};
use proflow::workflow::ResolvedBackground;
use serde::Deserialize;
use tokio::sync::Mutex;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionFile {
    #[serde(default)]
    skip_output_keys: Vec<String>,
    #[serde(default)]
    overrides: Vec<DecisionOverride>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionOverride {
    output_key: String,
    action: Option<DecisionAction>,
    slide_type: Option<OverrideSlideType>,
    playlist_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum DecisionAction {
    UseExisting {
        file_path: String,
        arrangement: Option<String>,
    },
    EditDescription {
        file_path: String,
        background: Option<BackgroundId>,
    },
    GenerateNew {
        background: Option<BackgroundId>,
    },
    SetBackground {
        background: BackgroundId,
    },
    SelectArrangement {
        arrangement: String,
    },
}

struct BuildCliArgs {
    plan_id: String,
    service_name: String,
    playlist_name: Option<String>,
    skip_output_keys: Vec<String>,
    overrides: Vec<DecisionOverride>,
    playlist_export_mode: PlaylistExportMode,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = parse_args()?;
    let BuildCliArgs {
        plan_id,
        service_name,
        playlist_name,
        skip_output_keys,
        overrides: pending_overrides,
        playlist_export_mode,
    } = cli;

    let config = Config::load()?;
    let project_data_root = BuildLocations::discover_project_data_root()?;
    let mappings = load_project_config(&project_data_root.join(PROJECT_CONFIG_FILE))?;
    let locations = BuildLocations::discover(&mappings.defaults().library)?;
    let overrides = pending_overrides
        .into_iter()
        .map(|pending| resolve_decision_override(pending, &mappings))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let pco_client = PlanningCenterClient::new(&config)?;
    let bible_service = Arc::new(Mutex::new(BibleService::new(
        locations.project_data_root().join("bibles"),
    )));
    let file_index = Arc::new(Mutex::new(LibraryCatalog::build(
        locations.presentation_library(),
    )?));
    let playlist_metadata =
        PlaylistMetadata::read_from_propresenter_root(locations.propresenter_root())?;
    let render_assets = RenderAssetSnapshot::load(mappings, locations)?;

    let executor = ServiceBuildExecutor::new(
        &pco_client,
        &bible_service,
        &file_index,
        &render_assets,
        &playlist_metadata,
    );

    let result = executor
        .build_service(&BuildRequest {
            plan_id,
            service_name: Some(service_name),
            playlist_name,
            skip_output_keys,
            overrides,
            playlist_export: match playlist_export_mode {
                PlaylistExportMode::LibraryLinks => PlaylistExportIntent::library_links(),
                PlaylistExportMode::PortableImport => {
                    PlaylistExportIntent::portable_import(Vec::new())
                }
            },
        })
        .await?;

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn parse_args() -> anyhow::Result<BuildCliArgs> {
    let mut args = std::env::args().skip(1);
    let plan_id = args
        .next()
        .context("usage: build_service <plan_id> <service_name> [playlist_name] [options]")?;
    let service_name = args
        .next()
        .context("usage: build_service <plan_id> <service_name> [playlist_name] [options]")?;
    let mut playlist_name: Option<String> = None;
    let mut skip_output_keys = Vec::new();
    let mut overrides = Vec::new();
    let mut playlist_export_mode = PlaylistExportMode::default();
    let mut package_mode_was_set = false;

    while let Some(arg) = args.next() {
        if arg == "--skip" {
            skip_output_keys.push(args.next().context("--skip requires an output_key")?);
        } else if arg == "--decisions" {
            let path = args
                .next()
                .context("--decisions requires a JSON file path")?;
            let decisions = read_decision_file(&path)?;
            skip_output_keys.extend(decisions.skip_output_keys);
            overrides.extend(decisions.overrides);
        } else if arg == "--override-file" {
            let output_key = args
                .next()
                .context("--override-file requires an output_key")?;
            let file_path = args
                .next()
                .context("--override-file requires a file path")?;
            let slide_type = args
                .next()
                .context("--override-file requires a slide type")?
                .parse::<OverrideSlideType>()
                .map_err(anyhow::Error::msg)?;
            let file_path = expand_path(&file_path);
            let playlist_name = file_path
                .file_stem()
                .and_then(|name| name.to_str())
                .map(ToString::to_string);
            overrides.push(DecisionOverride {
                output_key,
                action: Some(DecisionAction::UseExisting {
                    file_path: file_path.display().to_string(),
                    arrangement: None,
                }),
                playlist_name,
                slide_type: Some(slide_type),
            });
        } else if matches!(arg.as_str(), "--portable" | "--library-local") {
            if package_mode_was_set {
                anyhow::bail!("package mode may be specified only once");
            }
            playlist_export_mode = if arg == "--portable" {
                PlaylistExportMode::PortableImport
            } else {
                PlaylistExportMode::LibraryLinks
            };
            package_mode_was_set = true;
        } else if playlist_name.is_none() {
            playlist_name = Some(arg);
        } else {
            anyhow::bail!("unexpected argument: {arg}");
        }
    }

    Ok(BuildCliArgs {
        plan_id,
        service_name,
        playlist_name,
        skip_output_keys,
        overrides,
        playlist_export_mode,
    })
}

fn read_decision_file(path: &str) -> anyhow::Result<DecisionFile> {
    let path = expand_path(path);
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn expand_path(path: &str) -> PathBuf {
    expand_user_path(path)
}

fn resolve_decision_override(
    value: DecisionOverride,
    config: &ProjectConfig,
) -> anyhow::Result<EntryOverride> {
    let action = value
        .action
        .map(|action| resolve_decision_action(action, config))
        .transpose()?;

    Ok(EntryOverride {
        output_key: value.output_key,
        playlist_name: value.playlist_name,
        slide_type: value.slide_type,
        action,
    })
}

fn resolve_decision_action(
    action: DecisionAction,
    config: &ProjectConfig,
) -> anyhow::Result<OverrideAction> {
    match action {
        DecisionAction::UseExisting {
            file_path,
            arrangement,
        } => Ok(OverrideAction::UseExisting {
            file_path: expand_path(&file_path),
            arrangement,
        }),
        DecisionAction::EditDescription {
            file_path,
            background,
        } => Ok(OverrideAction::EditDescription {
            file_path: expand_path(&file_path),
            background: background
                .map(|id| resolve_background(id, config))
                .transpose()?,
        }),
        DecisionAction::GenerateNew { background } => Ok(OverrideAction::GenerateNew {
            background: background
                .map(|id| resolve_background(id, config))
                .transpose()?,
        }),
        DecisionAction::SetBackground { background } => Ok(OverrideAction::SetBackground {
            background: resolve_background(background, config)?,
        }),
        DecisionAction::SelectArrangement { arrangement } => {
            Ok(OverrideAction::SelectArrangement { arrangement })
        }
    }
}

fn resolve_background(
    id: BackgroundId,
    config: &ProjectConfig,
) -> anyhow::Result<ResolvedBackground> {
    let asset = config
        .backgrounds()
        .get(&id)
        .cloned()
        .with_context(|| format!("override references unknown background '{id}'"))?;
    Ok(ResolvedBackground::new(id, asset))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use proflow::project_config::BackgroundAssetPath;

    use super::*;

    fn pending_background_override(id: BackgroundId) -> DecisionOverride {
        DecisionOverride {
            output_key: "item:1".to_string(),
            action: Some(DecisionAction::SetBackground { background: id }),
            slide_type: None,
            playlist_name: None,
        }
    }

    #[test]
    fn resolves_background_override_through_project_registry() {
        let id = BackgroundId::new("communion").expect("valid test background id");
        let asset = BackgroundAssetPath::new("backgrounds/communion.png")
            .expect("valid test background path");
        let mut raw = RawProjectConfig::default();
        raw.backgrounds.insert(id.clone(), asset.clone());
        let config = ProjectConfig::try_from(raw).expect("valid runtime config");

        let resolved = resolve_decision_override(pending_background_override(id.clone()), &config)
            .expect("registered background should resolve");
        let background = resolved
            .action
            .and_then(|action| match action {
                OverrideAction::SetBackground { background } => Some(background),
                _ => None,
            })
            .expect("background should be present");

        assert_eq!(background.id(), &id);
        assert_eq!(background.file(), &asset);
    }

    #[test]
    fn rejects_unregistered_background_override() {
        let id = BackgroundId::new("missing").expect("valid test background id");
        let config = ProjectConfig::try_from(RawProjectConfig::default())
            .expect("valid empty runtime config");
        let error = resolve_decision_override(pending_background_override(id), &config)
            .expect_err("unregistered background should fail");

        assert!(error
            .to_string()
            .contains("override references unknown background 'missing'"));
    }
}
