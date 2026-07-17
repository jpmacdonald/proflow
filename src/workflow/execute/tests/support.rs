use super::*;

pub(super) fn test_background(id: &str) -> ResolvedBackground {
    ResolvedBackground::new(
        BackgroundId::new(id).expect("valid background id"),
        BackgroundAssetPath::new(format!("backgrounds/{id}.png")).expect("valid background path"),
    )
}

pub(super) fn test_macro_transitions() -> crate::workflow::plan::RestyleMacroPolicy {
    crate::workflow::plan::RestyleMacroPolicy::new(vec![
        crate::workflow::plan::RestyleMacroRegion::new(
            crate::workflow::plan::RestyleMacroSelector::OperatorCue { index: 0 },
            "Song".to_string(),
        )
        .expect("valid test macro region"),
    ])
    .expect("valid test macro policy")
}

pub(super) fn test_transform() -> crate::workflow::plan::ExistingTransform {
    crate::workflow::plan::ExistingTransform::new(
        crate::workflow::plan::BackgroundTransform::Replace(test_background("default")),
        crate::workflow::plan::MacroTransform::Enforce(test_macro_transitions()),
        crate::workflow::plan::CueTransform::Preserve,
    )
    .expect("valid existing-presentation transform")
}

pub(super) fn minimal_png(width: u32, height: u32) -> Vec<u8> {
    let pixel_count = usize::try_from(width).expect("width fits usize")
        * usize::try_from(height).expect("height fits usize");
    let pixels = vec![0u8; pixel_count * 4];
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&pixels, width, height, ColorType::Rgba8.into())
        .expect("encode PNG fixture");
    bytes
}

pub(super) fn test_style(background: Option<ResolvedBackground>) -> RenderStyle {
    RenderStyle::new(background, test_role(None), None, None).expect("valid render style")
}

pub(super) fn test_role(cue_macro: Option<CueMacro>) -> RenderRole {
    RenderRole::new(
        "content".to_string(),
        "Content".to_string(),
        BTreeMap::new(),
        cue_macro,
        None,
    )
    .expect("valid render role")
}

pub(super) fn parsed_content() -> ParsedContent {
    ParsedContent::new(
        vec![ParsedSegment {
            text: "Generated content".to_string(),
            speaker: SpeakerRole::Neutral,
            bold: None,
            italic: None,
        }],
        Some("Generated title".to_string()),
    )
}

pub(super) fn test_plan(output_key: &str, disposition: PlanDisposition) -> ResolvedItemPlan {
    ResolvedItemPlan {
        output_key: OutputKey::new(output_key.to_string()).expect("valid test output key"),
        position: 1,
        pco_title: "Test item".to_string(),
        playlist_name: "Test item".to_string(),
        reason: "Test fixture".to_string(),
        item_kind: ItemKind::Other,
        item_type: None,
        disposition,
    }
}

pub(super) fn use_existing_plan(output_key: &str, file_path: PathBuf) -> ResolvedItemPlan {
    test_plan(
        output_key,
        PlanDisposition::Ready(ReadyAction::UseExisting {
            file_path,
            arrangement: None,
        }),
    )
}

pub(super) fn generate_title_plan(output_key: &str, style: RenderStyle) -> ResolvedItemPlan {
    test_plan(
        output_key,
        PlanDisposition::Ready(ReadyAction::GenerateTitle {
            text: "Generated title".to_string(),
            style,
        }),
    )
}

pub(super) fn presentation_with_size(name: &str, width: f64, height: f64) -> rv_data::Presentation {
    let cue_id = Uuid::new_v4().to_string();
    let group_id = Uuid::new_v4().to_string();
    rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        name: name.to_string(),
        cues: vec![rv_data::Cue {
            uuid: Some(rv_data::Uuid {
                string: cue_id.clone(),
            }),
            actions: vec![rv_data::Action {
                action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                    rv_data::action::SlideType {
                        slide: Some(rv_data::action::slide_type::Slide::Presentation(
                            rv_data::PresentationSlide {
                                base_slide: Some(rv_data::Slide {
                                    size: Some(rv_data::graphics::Size { width, height }),
                                    ..rv_data::Slide::default()
                                }),
                                ..rv_data::PresentationSlide::default()
                            },
                        )),
                    },
                )),
                ..rv_data::Action::default()
            }],
            ..rv_data::Cue::default()
        }],
        cue_groups: vec![rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid { string: group_id }),
                ..rv_data::Group::default()
            }),
            cue_identifiers: vec![rv_data::Uuid { string: cue_id }],
        }],
        ..rv_data::Presentation::default()
    }
}

pub(super) fn fixture_template_slide() -> rv_data::PresentationSlide {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter/native/templates/scripture-template.pro");
    let bytes = std::fs::read(path).expect("read template fixture");
    let presentation = rv_data::Presentation::decode(bytes.as_slice()).expect("decode fixture");
    presentation
        .cues
        .iter()
        .flat_map(|cue| &cue.actions)
        .find_map(|action| match &action.action_type_data {
            Some(rv_data::action::ActionTypeData::Slide(slide)) => match &slide.slide {
                Some(rv_data::action::slide_type::Slide::Presentation(slide)) => {
                    Some(slide.clone())
                }
                _ => None,
            },
            _ => None,
        })
        .expect("fixture presentation slide")
}

pub(super) fn install_fixture_theme(runtime: &mut TestRuntime) {
    let theme_dir = runtime.locations().themes().join("Execution Test Theme");
    std::fs::create_dir_all(&theme_dir).expect("create theme directory");
    let theme = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: fixture_template_slide().base_slide,
            name: "Content".to_string(),
            actions: Vec::new(),
        }],
        ..rv_data::template::Document::default()
    };
    std::fs::write(theme_dir.join("Theme"), theme.encode_to_vec()).expect("write theme");
    runtime.select_theme("Execution Test Theme");
}

pub(super) struct TestRuntime {
    pub(super) pco_client: PlanningCenterClient,
    pub(super) bible_service: Arc<Mutex<BibleService>>,
    pub(super) file_index: Arc<Mutex<LibraryCatalog>>,
    pub(super) render_assets: RenderAssetSnapshot,
    pub(super) playlist_metadata: PlaylistMetadata,
}

impl TestRuntime {
    pub(super) fn new(root: &Path) -> Self {
        let data = root.join("data");
        let output = root.join("output");
        let propresenter = root.join("ProPresenter");
        std::fs::create_dir_all(data.join("bibles")).expect("create test data");
        std::fs::create_dir_all(&output).expect("create output directory");
        std::fs::create_dir_all(&propresenter).expect("create ProPresenter root");
        let locations = BuildLocations::from_inputs(BuildLocationInputs {
            project_data_root: data.clone(),
            presentation_library: output.clone(),
            playlist_output: output.clone(),
            propresenter_root: propresenter.clone(),
            themes: propresenter.join("Themes"),
            macros: propresenter.join("Configuration/Macros"),
        })
        .expect("checked test locations");
        let pco_client = PlanningCenterClient::new(&crate::config::Config {
            pco_app_id: "test-app".to_string(),
            pco_secret: "test-secret".to_string(),
        })
        .expect("test Planning Center client settings are valid");
        let render_assets = RenderAssetSnapshot::load(
            ProjectConfig::try_from(crate::project_config::RawProjectConfig::default())
                .expect("valid empty project config"),
            locations,
        )
        .expect("load empty test render assets");
        Self {
            pco_client,
            bible_service: Arc::new(Mutex::new(BibleService::new(data.join("bibles")))),
            file_index: Arc::new(Mutex::new(
                LibraryCatalog::build(&output).expect("build empty test library catalog"),
            )),
            render_assets,
            playlist_metadata: PlaylistMetadata::offline_test(),
        }
    }

    pub(super) const fn locations(&self) -> &BuildLocations {
        self.render_assets.locations()
    }

    pub(super) fn select_theme(&mut self, name: &str) {
        let mut raw = self.render_assets.config().as_raw().clone();
        raw.defaults.theme = Some(name.to_string());
        let config = ProjectConfig::try_from(raw).expect("valid test project config");
        let locations = self.render_assets.locations().clone();
        self.render_assets = RenderAssetSnapshot::load(config, locations)
            .expect("load configured test render assets");
    }

    pub(super) fn reload_render_assets(&mut self) {
        let config = self.render_assets.config().clone();
        let locations = self.render_assets.locations().clone();
        self.render_assets =
            RenderAssetSnapshot::load(config, locations).expect("reload test render assets");
    }

    pub(super) fn replace_locations(&mut self, locations: BuildLocations) {
        let config = self.render_assets.config().clone();
        self.render_assets = RenderAssetSnapshot::load(config, locations)
            .expect("load test render assets from replacement locations");
    }

    pub(super) fn executor(&self) -> ServiceBuildExecutor<'_> {
        ServiceBuildExecutor::new(
            &self.pco_client,
            &self.bible_service,
            &self.file_index,
            &self.render_assets,
            &self.playlist_metadata,
        )
    }
}

pub(super) fn reviewed_request(playlist_name: &str) -> BuildRequest {
    BuildRequest {
        plan_id: "plan-1".to_string(),
        service_name: Some("Sunday Morning".to_string()),
        playlist_name: Some(playlist_name.to_string()),
        // Most execution tests isolate presentation or transaction behavior.
        // Portable-package tests opt in explicitly below their own fixtures.
        playlist_package_mode: PlaylistPackageMode::LibraryLocal,
        ..BuildRequest::default()
    }
}

pub(super) fn expect_prepared(review: BuildReview) -> PreparedBuildRequest {
    review
        .into_prepared()
        .expect("test review should contain exact prepared artifacts")
}
