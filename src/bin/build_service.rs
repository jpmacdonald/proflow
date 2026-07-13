//! Build a `ProPresenter` service playlist from a Planning Center plan.
//!
//! Usage:
//! ```text
//! cargo run --bin build_service -- <plan_id> <service_name> [playlist_name] [--skip <output_key> ...] [--decisions decisions.json] [--library-local]
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use proflow::bible::BibleService;
use proflow::config::Config;
use proflow::paths::{find_data_subdir, project_config_path};
use proflow::planning_center::PlanningCenterClient;
use proflow::project_config::{
    load_project_config, validate_project_config, BackgroundId, ProjectConfig,
};
use proflow::propresenter::macros::MacroCache;
use proflow::propresenter::package::PlaylistPackageMode;
use proflow::propresenter::playlist::PlaylistMetadata;
use proflow::propresenter::template::ThemeCache;
use proflow::utils::file_index::FileIndex;
use proflow::workflow::execute::{
    BuildRequest, EntryOverride, OverrideSlideType, ServiceBuildExecutor,
};
use proflow::workflow::{PlanAction, ResolvedBackground};
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
    file_path: Option<String>,
    slide_type: Option<OverrideSlideType>,
    playlist_name: Option<String>,
    background: Option<BackgroundId>,
    arrangement: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DecisionAction {
    UseExisting,
    EditInPlace,
    GenerateNew,
}

struct BuildCliArgs {
    plan_id: String,
    service_name: String,
    playlist_name: Option<String>,
    skip_output_keys: Vec<String>,
    overrides: Vec<DecisionOverride>,
    playlist_package_mode: PlaylistPackageMode,
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
        playlist_package_mode,
    } = cli;

    let config = Config::load()?;
    let mappings = load_project_config(&project_config_path())?;
    let issues = validate_project_config(&mappings);
    if !issues.is_empty() {
        for issue in &issues {
            eprintln!("Config error at {}: {}", issue.path, issue.message);
        }
        anyhow::bail!("config validation failed");
    }
    let overrides = pending_overrides
        .into_iter()
        .map(|pending| resolve_decision_override(pending, &mappings))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let library_path = env_path("LIBRARY_DIR")
        .or_else(proflow::utils::file_index::get_default_library_path)
        .context("LIBRARY_DIR or the default ProPresenter library path is required")?;
    let playlist_output_dir = env_path("PLAYLIST_DIR").or_else(|| Some(library_path.clone()));
    let generated_presentation_dir = env_path("GENERATED_PRESENTATIONS_DIR")
        .unwrap_or_else(|| default_generated_dir(&library_path));

    let pco_client = PlanningCenterClient::new(&config);
    let bible_service = Arc::new(Mutex::new(BibleService::new(find_data_subdir("bibles"))));
    let file_index = Arc::new(Mutex::new(Some(FileIndex::build(&library_path)?)));
    let template_cache = ThemeCache::load(mappings.defaults.theme.as_deref())?;
    let macro_cache = MacroCache::load_default()?;
    let playlist_metadata = PlaylistMetadata::read_from_library_dir(&library_path)?;

    let executor = ServiceBuildExecutor::new(
        &pco_client,
        &bible_service,
        &file_index,
        &template_cache,
        &macro_cache,
        &playlist_metadata,
        playlist_output_dir.as_deref(),
        Some(&generated_presentation_dir),
    );

    let result = executor
        .build_service(
            &BuildRequest {
                plan_id,
                service_name: Some(service_name),
                playlist_name,
                skip_output_keys,
                overrides,
                playlist_package_mode,
                media_assets: Vec::new(),
            },
            &mappings,
        )
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
    let mut playlist_package_mode = PlaylistPackageMode::ExportPortable;
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
                action: None,
                playlist_name,
                file_path: Some(file_path.display().to_string()),
                slide_type: Some(slide_type),
                background: None,
                arrangement: None,
            });
        } else if matches!(arg.as_str(), "--portable" | "--library-local") {
            if package_mode_was_set {
                anyhow::bail!("package mode may be specified only once");
            }
            playlist_package_mode = if arg == "--portable" {
                PlaylistPackageMode::ExportPortable
            } else {
                PlaylistPackageMode::LibraryLocal
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
        playlist_package_mode,
    })
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .map(|value| PathBuf::from(shellexpand::tilde(&value).to_string()))
}

fn read_decision_file(path: &str) -> anyhow::Result<DecisionFile> {
    let path = expand_path(path);
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

fn expand_path(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).to_string())
}

fn resolve_decision_override(
    value: DecisionOverride,
    config: &ProjectConfig,
) -> anyhow::Result<EntryOverride> {
    let file_path = value.file_path.map(|path| expand_path(&path));
    let playlist_name = value.playlist_name.or_else(|| {
        file_path
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|name| name.to_str())
            .map(ToString::to_string)
    });
    let background = if let Some(id) = value.background {
        let asset = config
            .backgrounds
            .get(&id)
            .cloned()
            .with_context(|| format!("override references unknown background '{id}'"))?;
        Some(ResolvedBackground::new(id, asset))
    } else {
        None
    };

    Ok(EntryOverride {
        output_key: value.output_key,
        action: value.action.map(PlanAction::from),
        playlist_name,
        file_path: file_path.map(|path| path.display().to_string()),
        slide_type: value.slide_type,
        background,
        arrangement: value.arrangement,
    })
}

impl From<DecisionAction> for PlanAction {
    fn from(value: DecisionAction) -> Self {
        match value {
            DecisionAction::UseExisting => Self::UseExisting,
            DecisionAction::EditInPlace => Self::EditInPlace,
            DecisionAction::GenerateNew => Self::GenerateNew,
        }
    }
}

fn default_generated_dir(library_path: &Path) -> PathBuf {
    let default_library = library_path.join("Default");
    if default_library.is_dir() {
        default_library
    } else {
        library_path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use proflow::project_config::BackgroundAssetPath;

    use super::*;

    fn pending_background_override(id: BackgroundId) -> DecisionOverride {
        DecisionOverride {
            output_key: "item:1".to_string(),
            action: None,
            file_path: None,
            slide_type: None,
            playlist_name: None,
            background: Some(id),
            arrangement: None,
        }
    }

    #[test]
    fn resolves_background_override_through_project_registry() {
        let id = BackgroundId::new("communion").expect("valid test background id");
        let asset = BackgroundAssetPath::new("backgrounds/communion.png")
            .expect("valid test background path");
        let mut config = ProjectConfig::default();
        config.backgrounds.insert(id.clone(), asset.clone());

        let resolved = resolve_decision_override(pending_background_override(id.clone()), &config)
            .expect("registered background should resolve");
        let background = resolved.background.expect("background should be present");

        assert_eq!(background.id(), &id);
        assert_eq!(background.file(), &asset);
    }

    #[test]
    fn rejects_unregistered_background_override() {
        let id = BackgroundId::new("missing").expect("valid test background id");
        let error =
            resolve_decision_override(pending_background_override(id), &ProjectConfig::default())
                .expect_err("unregistered background should fail");

        assert!(error
            .to_string()
            .contains("override references unknown background 'missing'"));
    }
}
