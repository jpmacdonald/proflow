use proflow::propresenter::{
    data_model as dm,
    generated::rv_data::{
        self,
        graphics,
    },
    PresentationBuilder,
};
use uuid::Uuid;
use prost::Message;
use std::path::PathBuf;
use std::fs::File;
use std::io::Write;

fn create_text_element(content: &str, element_uuid: Uuid) -> dm::Element {
    dm::Element::Text(dm::TextElement {
        content: content.to_string(),
        bounds: Some(dm::Rect {
            origin: dm::Point { x: 17.8, y: 290.0 },
            size: dm::Size { width: 1884.4, height: 500.0 },
        }),
        color: dm::Color { red: 1.0, green: 1.0, blue: 1.0, alpha: 1.0 },
        font: dm::Font {
            name: "Helvetica".to_string(),
            size: 80.0,
            italic: false,
            bold: false,
            family: "Helvetica".to_string(),
            face: "".to_string(),
        },
        shadow: Some(dm::Shadow {
            enable: true,
            style: dm::ShadowStyle::Drop,
            angle: 315.0,
            offset: dm::Point { x: 5.0, y: 5.0 },
            radius: 5.0,
            color: dm::Color { red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0 },
            opacity: 0.9,
        }),
        paragraph_style: dm::ParagraphStyle {
            alignment: dm::TextAlignment::Center,
            first_line_head_indent: 0.0,
            head_indent: 0.0,
            tail_indent: 0.0,
            line_height_multiple: 1.5,
            maximum_line_height: 0.0,
            minimum_line_height: 0.0,
            line_spacing: 0.0,
            paragraph_spacing: 0.0,
            paragraph_spacing_before: 0.0,
            tab_stops: vec![],
            default_tab_interval: 0.0,
            text_list: Some(dm::TextList {
                number_type: dm::TextListNumberType::None,
                prefix: "".to_string(),
                suffix: "".to_string(),
                start_index: 0,
                indent: 0.0,
                indent_level: 0,
            }),
            text_lists: vec![],
        },
        custom_attributes: vec![
            dm::CustomAttribute {
                range: dm::Range { start: 0, end: content.len() as u32 },
                attribute: dm::CustomAttributeType::OriginalFontSize(80.0),
            },
            dm::CustomAttribute {
                range: dm::Range { start: 0, end: content.len() as u32 },
                attribute: dm::CustomAttributeType::FontScaleFactor(0.6),
            },
        ],
        mask: Some(dm::LineFillMask {
            fill_type: dm::LineFillType::None,
            color: None,
            gradient: None,
            angle: 0.0,
            spacing: 0.0,
            width: 0.0,
            phase: 0.0,
        }),
        text_scroller: Some(dm::TextScroller {
            should_scroll: false,
            scroll_rate: 0.5,
            should_repeat: true,
            repeat_distance: 0.07959997477781541,
            scrolling_direction: dm::ScrollDirection::Left,
            starts_off_screen: false,
            fade_left: 0.0,
            fade_right: 0.0,
        }),
    })
}

fn create_slide(content: &str, slide_uuid: Uuid, element_uuid: Uuid) -> dm::Slide {
    dm::Slide {
        base: dm::BaseSlide {
            uuid: slide_uuid,
            elements: vec![create_text_element(content, element_uuid)],
            element_build_order: vec![],
            guidelines: vec![],
            draws_background_color: false,
            background_color: Some(dm::Color { red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0 }),
            size: dm::Size { width: 1920.0, height: 1080.0 },
        },
        notes: Some("".to_string()),
        template_guidelines: vec![],
        chord_chart: Some(dm::Url { url: "".to_string() }),
        transition: None,
    }
}

fn create_media_action(name: &str, path: &str) -> dm::Action {
    dm::Action::Media {
        uuid: Uuid::new_v4(),
        name: name.to_string(),
        source: dm::MediaSource::File(PathBuf::from(path)),
        fit: dm::MediaFit::Scale,
        opacity: 1.0,
        volume: 1.0,
        delay_time: 0.0,
        duration: 0.0,
        enabled: true,
    }
}

fn create_macro_action(name: &str, uuid: Uuid, parent_uuid: Uuid) -> dm::Action {
    dm::Action::Macro {
        uuid: Uuid::new_v4(),
        name: name.to_string(),
        parameter_uuid: uuid,
        parameter_name: name.to_string(),
        parent_collection_uuid: Some(parent_uuid),
        parent_collection_name: Some("Default Collection".to_string()),
        delay_time: 0.0,
        duration: 0.0,
        enabled: true,
    }
}

fn build_presentation_manually(
    name: String,
    uuid: Uuid,
    category: String,
    cues: Vec<dm::Cue>,
    cue_groups: Vec<dm::CueGroup>,
    arrangements: Vec<dm::Arrangement>,
    selected_arrangement: Uuid,
) -> dm::Presentation {
    // Build presentation without modifying the cue fields
    dm::Presentation {
        name,
        uuid,
        category,
        notes: String::new(),
        ccli: None,
        bible_reference: None,
        cues,
        cue_groups,
        arrangements,
        timeline: None,
        application_info: None,
        music_key: String::new(),
        music: None,
        slide_show: None,
        path: None,
        last_used: None,
        last_modified: None,
    }
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create UUIDs for all components
    // Presentation UUID - unique for this presentation
    let presentation_uuid = Uuid::new_v4();
    
    // Group UUIDs - unique for each group
    let verse1_group_uuid = Uuid::new_v4();
    let verse2_group_uuid = Uuid::new_v4();
    let verse3_group_uuid = Uuid::new_v4();
    let verse4_group_uuid = Uuid::new_v4();
    let verse5_group_uuid = Uuid::new_v4();
    let blank_group_uuid = Uuid::new_v4();
    let title_group_uuid = Uuid::new_v4();

    // Application group UUIDs - strings
    let verse1_app_group_id = Uuid::new_v4().to_string();
    let verse2_app_group_id = Uuid::new_v4().to_string();
    let verse3_app_group_id = Uuid::new_v4().to_string();
    let verse4_app_group_id = Uuid::new_v4().to_string();
    let verse5_app_group_id = Uuid::new_v4().to_string();
    let blank_app_group_id = Uuid::new_v4().to_string();
    let title_app_group_id = "".to_string();

    // Cue UUIDs
    let verse1_cue_uuid = Uuid::new_v4();
    let verse2_cue_uuid = Uuid::new_v4();
    let verse3_cue_uuid = Uuid::new_v4();
    let verse4_cue_uuid = Uuid::new_v4();
    let verse5_cue_uuid = Uuid::new_v4();
    let blank_cue_uuid = Uuid::new_v4();
    let title_cue_uuid = Uuid::new_v4();

    // Action UUIDs
    let verse1_action_uuid = Uuid::new_v4();
    let verse2_action_uuid = Uuid::new_v4();
    let verse3_action_uuid = Uuid::new_v4();
    let verse4_action_uuid = Uuid::new_v4();
    let verse5_action_uuid = Uuid::new_v4();
    let blank_action_uuid = Uuid::new_v4();
    let title_action_uuid = Uuid::new_v4();

    // Slide UUIDs - IMPORTANT: These must be preserved through the conversion process
    let verse1_slide_uuid = Uuid::new_v4();
    let verse2_slide_uuid = Uuid::new_v4();
    let verse3_slide_uuid = Uuid::new_v4();
    let verse4_slide_uuid = Uuid::new_v4();
    let verse5_slide_uuid = Uuid::new_v4();
    let blank_slide_uuid = Uuid::new_v4();
    let title_slide_uuid = Uuid::new_v4();

    // Element UUIDs
    let verse1_element_uuid = Uuid::new_v4();
    let verse2_element_uuid = Uuid::new_v4();
    let verse3_element_uuid = Uuid::new_v4();
    let verse4_element_uuid = Uuid::new_v4();
    let verse5_element_uuid = Uuid::new_v4();
    let blank_element_uuid = Uuid::new_v4();
    let title_element_uuid = Uuid::new_v4();

    // Other UUIDs needed for the presentation
    let slide_layer_uuid = Uuid::new_v4();
    let arrangement_uuid = Uuid::new_v4();

    // Create slides
    let title_slide = create_slide(
        "Amazing Grace, How Sweet The Sound",
        title_slide_uuid,
        title_element_uuid
    );

    let verse1_slide = create_slide(
        "Amazing grace! How sweet the sound\n\
That saved a wretch like me!\n\
I once was lost, but now am found;\n\
Was blind, but now I see.",
        verse1_slide_uuid,
        verse1_element_uuid
    );

    let verse2_slide = create_slide(
        "'Twas grace that taught my heart to fear,\n\
And grace my fears relieved;\n\
How precious did that grace appear\n\
The hour I first believed.",
        verse2_slide_uuid,
        verse2_element_uuid
    );

    let verse3_slide = create_slide(
        "Through many dangers, toils and snares,\n\
I have already come;\n\
'Tis grace hath brought me safe thus far,\n\
And grace will lead me home.",
        verse3_slide_uuid,
        verse3_element_uuid
    );

    let verse4_slide = create_slide(
        "When we've been there ten thousand years\n\
Bright shining as the sun,\n\
We've no less days to sing God's praise\n\
Than when we'd first begun.",
        verse4_slide_uuid,
        verse4_element_uuid
    );

    let verse5_slide = create_slide(
        "The Lord has promised good to me,\n\
His Word my hope secures;\n\
He will my Shield and Portion be,\n\
As long as life endures.",
        verse5_slide_uuid,
        verse5_element_uuid
    );

    let blank_slide = create_slide(
        "",
        blank_slide_uuid,
        blank_element_uuid
    );

    // Create layer identification
    let slide_layer = dm::LayerIdentification {
        uuid: slide_layer_uuid,
        name: "Slide Layer".to_string(),
    };

    // Create cues - each cue should have only one action
    let verse1_cue = dm::Cue {
        uuid: verse1_cue_uuid,
        name: "".to_string(),
        actions: vec![
            dm::Action::Slide {
                uuid: verse1_action_uuid,
                name: "".to_string(),
                slide: verse1_slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            },
        ],
        enabled: true,
        hot_key: Some(dm::HotKey {
            key_code: 1, // AnsiA = 1
            modifiers: 0,
            control_identifier: String::new(),
        }),
        completion_target_type: dm::CompletionTargetType::None,
        completion_target_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_action_type: dm::CompletionActionType::First,
        completion_action_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_time: 0.0,
    };

    let verse2_cue = dm::Cue {
        uuid: verse2_cue_uuid,
        name: "".to_string(),
        actions: vec![
            dm::Action::Slide {
                uuid: verse2_action_uuid,
                name: "".to_string(),
                slide: verse2_slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            },
        ],
        enabled: true,
        hot_key: Some(dm::HotKey {
            key_code: 19, // AnsiS = 19
            modifiers: 0,
            control_identifier: String::new(),
        }),
        completion_target_type: dm::CompletionTargetType::None,
        completion_target_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_action_type: dm::CompletionActionType::First,
        completion_action_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_time: 0.0,
    };

    let verse3_cue = dm::Cue {
        uuid: verse3_cue_uuid,
        name: "".to_string(),
        actions: vec![
            dm::Action::Slide {
                uuid: verse3_action_uuid,
                name: "".to_string(),
                slide: verse3_slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            },
        ],
        enabled: true,
        hot_key: Some(dm::HotKey {
            key_code: 4, // AnsiD = 4
            modifiers: 0,
            control_identifier: String::new(),
        }),
        completion_target_type: dm::CompletionTargetType::None,
        completion_target_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_action_type: dm::CompletionActionType::First,
        completion_action_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_time: 0.0,
    };

    let verse4_cue = dm::Cue {
        uuid: verse4_cue_uuid,
        name: "".to_string(),
        actions: vec![
            dm::Action::Slide {
                uuid: verse4_action_uuid,
                name: "".to_string(),
                slide: verse4_slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            },
        ],
        enabled: true,
        hot_key: Some(dm::HotKey {
            key_code: 6, // AnsiF = 6
            modifiers: 0,
            control_identifier: String::new(),
        }),
        completion_target_type: dm::CompletionTargetType::None,
        completion_target_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_action_type: dm::CompletionActionType::First,
        completion_action_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_time: 0.0,
    };
    
    let verse5_cue = dm::Cue {
        uuid: verse5_cue_uuid,
        name: "".to_string(),
        actions: vec![
            dm::Action::Slide {
                uuid: verse5_action_uuid,
                name: "".to_string(),
                slide: verse5_slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            },
        ],
        enabled: true,
        hot_key: Some(dm::HotKey {
            key_code: 7, // AnsiG = 7
            modifiers: 0,
            control_identifier: String::new(),
        }),
        completion_target_type: dm::CompletionTargetType::None,
        completion_target_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_action_type: dm::CompletionActionType::First,
        completion_action_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_time: 0.0,
    };

    let blank_cue = dm::Cue {
        uuid: blank_cue_uuid,
        name: "".to_string(),
        actions: vec![
            dm::Action::Slide {
                uuid: blank_action_uuid,
                name: "".to_string(),
                slide: blank_slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            },
        ],
        enabled: true,
        hot_key: None,
        completion_target_type: dm::CompletionTargetType::None,
        completion_target_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_action_type: dm::CompletionActionType::First,
        completion_action_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_time: 0.0,
    };

    let title_cue = dm::Cue {
        uuid: title_cue_uuid,
        name: "".to_string(),
        actions: vec![
            dm::Action::Slide {
                uuid: title_action_uuid,
                name: "".to_string(),
                slide: title_slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            },
        ],
        enabled: true,
        hot_key: None,
        completion_target_type: dm::CompletionTargetType::None,
        completion_target_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_action_type: dm::CompletionActionType::First,
        completion_action_uuid: Some(Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap()),
        completion_time: 0.0,
    };

    // Create cue groups with proper UUIDs and colors
    let verse1_group = dm::CueGroup {
        group: dm::Group {
            uuid: verse1_group_uuid,
            name: "Verse 1".to_string(),
            color: dm::Color { red: 0.0, green: 0.46666667, blue: 0.8, alpha: 1.0 },
            hot_key: Some(dm::HotKey {
                key_code: 1, // AnsiA = 1
                modifiers: 0,
                control_identifier: String::new(),
            }),
            application_group_identifier: verse1_app_group_id,
        },
        cue_identifiers: vec![verse1_cue_uuid],
    };

    let verse2_group = dm::CueGroup {
        group: dm::Group {
            uuid: verse2_group_uuid,
            name: "Verse 2".to_string(),
            color: dm::Color { red: 0.0, green: 0.34901962, blue: 0.6, alpha: 1.0 },
            hot_key: Some(dm::HotKey {
                key_code: 19, // AnsiS = 19
                modifiers: 0,
                control_identifier: String::new(),
            }),
            application_group_identifier: verse2_app_group_id,
        },
        cue_identifiers: vec![verse2_cue_uuid],
    };

    let verse3_group = dm::CueGroup {
        group: dm::Group {
            uuid: verse3_group_uuid,
            name: "Verse 3".to_string(),
            color: dm::Color { red: 0.0, green: 0.23529412, blue: 0.4, alpha: 1.0 },
            hot_key: Some(dm::HotKey {
                key_code: 4, // AnsiD = 4
                modifiers: 0,
                control_identifier: String::new(),
            }),
            application_group_identifier: verse3_app_group_id,
        },
        cue_identifiers: vec![verse3_cue_uuid],
    };

    let verse4_group = dm::CueGroup {
        group: dm::Group {
            uuid: verse4_group_uuid,
            name: "Verse 4".to_string(),
            color: dm::Color { red: 0.0, green: 0.40784314, blue: 0.7019608, alpha: 1.0 },
            hot_key: Some(dm::HotKey {
                key_code: 6, // AnsiF = 6
                modifiers: 0,
                control_identifier: String::new(),
            }),
            application_group_identifier: verse4_app_group_id,
        },
        cue_identifiers: vec![verse4_cue_uuid],
    };

    let verse5_group = dm::CueGroup {
        group: dm::Group {
            uuid: verse5_group_uuid,
            name: "Verse 5".to_string(),
            color: dm::Color { red: 0.0, green: 0.2901961, blue: 0.5019608, alpha: 1.0 },
            hot_key: Some(dm::HotKey {
                key_code: 7, // AnsiG = 7
                modifiers: 0,
                control_identifier: String::new(),
            }),
            application_group_identifier: verse5_app_group_id,
        },
        cue_identifiers: vec![verse5_cue_uuid],
    };

    let blank_group = dm::CueGroup {
        group: dm::Group {
            uuid: blank_group_uuid,
            name: "Blank".to_string(),
            color: dm::Color { red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0 },
            hot_key: None,
            application_group_identifier: blank_app_group_id,
        },
        cue_identifiers: vec![blank_cue_uuid],
    };

    let title_group = dm::CueGroup {
        group: dm::Group {
            uuid: title_group_uuid,
            name: "Title".to_string(),
            color: dm::Color { red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0 },
            hot_key: None,
            application_group_identifier: title_app_group_id,
        },
        cue_identifiers: vec![title_cue_uuid],
    };

    // Create arrangement with proper order - using the exact order from the original file
    let arrangement = dm::Arrangement {
        uuid: arrangement_uuid,
        name: "Default".to_string(),
        group_identifiers: vec![
            verse1_group_uuid,  // Put the verses first
            verse2_group_uuid,
            verse3_group_uuid,
            verse4_group_uuid,
            verse5_group_uuid,
            blank_group_uuid,
            title_group_uuid,   // Put the title last
        ],
    };

    // Build presentation manually instead of using the builder
    let mut presentation = build_presentation_manually(
        "[Hymn] Amazing Grace".to_string(),
        presentation_uuid,
        "".to_string(),
        vec![
            // Order the cues the same way as in the arrangement
            verse1_cue, 
            verse2_cue, 
            verse3_cue, 
            verse4_cue, 
            verse5_cue, 
            blank_cue, 
            title_cue
        ],
        vec![
            // Order the groups the same way as in the arrangement
            verse1_group, 
            verse2_group, 
            verse3_group, 
            verse4_group, 
            verse5_group, 
            blank_group, 
            title_group
        ],
        vec![arrangement.clone()],
        arrangement_uuid,
    );

    // Set additional presentation properties
    presentation.ccli = Some(dm::CCLIInfo {
        author: "Edwin Othello Excell; John Newton; John P. Rees; William W. Walker".to_string(),
        artist_credits: "".to_string(),
        song_title: "Amazing Grace".to_string(),
        publisher: "Words: Public Domain; Music: Public Domain".to_string(),
        copyright_year: 0,
        song_number: 22025,
        display: true,
        album: "".to_string(),
    });

    presentation.timeline = Some(dm::Timeline {
        duration: 300.0,
        loop_enabled: false,
        timecode_enabled: false,
        timecode_offset: 0.0,
        cues: vec![],
    });

    // Convert to rv_data format and save
    let mut rv_presentation: rv_data::Presentation = presentation.into();

    // Set correct platform and application versions
    rv_presentation.application_info = Some(rv_data::ApplicationInfo {
        platform: rv_data::application_info::Platform::Windows as i32,
        platform_version: Some(rv_data::Version {
            major_version: 10,
            minor_version: 0,
            patch_version: 4294967295,
            build: "22621".to_string(),
        }),
        application: rv_data::application_info::Application::Propresenter as i32,
        application_version: Some(rv_data::Version {
            major_version: 17,
            minor_version: 1,
            patch_version: 0,
            build: "285278217".to_string(),
        }),
    });

    // After creating the presentation and before saving it
    // Modify the direct rv_data structure to set paths for all text elements
    for cue in rv_presentation.cues.iter_mut() {
        for action in cue.actions.iter_mut() {
            if let Some(rv_data::action::ActionTypeData::Slide(slide_type)) = &mut action.action_type_data {
                if let Some(rv_data::action::slide_type::Slide::Presentation(presentation_slide)) = &mut slide_type.slide {
                    if let Some(base_slide) = &mut presentation_slide.base_slide {
                        for element in base_slide.elements.iter_mut() {
                            if let Some(graphics_element) = &mut element.element {
                                // Change the element name to "Lyrics" to match original
                                graphics_element.name = "Lyrics".to_string();
                                
                                // Set the stroke color to white (1.0, 1.0, 1.0)
                                if let Some(stroke) = &mut graphics_element.stroke {
                                    if let Some(color) = &mut stroke.color {
                                        color.red = 1.0;
                                        color.green = 1.0;
                                        color.blue = 1.0;
                                    }
                                }
                                
                                // Set shadow enable to true
                                if let Some(shadow) = &mut graphics_element.shadow {
                                    shadow.enable = true;
                                }
                                
                                // Set the scale behavior to AdjustContainerHeight
                                if let Some(text) = &mut graphics_element.text {
                                    text.scale_behavior = rv_data::graphics::text::ScaleBehavior::AdjustContainerHeight as i32;
                                }
                                
                                // Try a completely different approach: replace the RTF data entirely
                                if let Some(text) = &mut graphics_element.text {
                                    // Get the text content
                                    let rtf_string = String::from_utf8_lossy(&text.rtf_data).to_string();
                                    
                                    // Extract any text content from the RTF
                                    let content_start = rtf_string.find("\\cf2").map(|pos| pos + 4).unwrap_or(0);
                                    let content_end = rtf_string.rfind("}").unwrap_or(rtf_string.len());
                                    let content = if content_start < content_end {
                                        &rtf_string[content_start..content_end]
                                    } else {
                                        ""
                                    };
                                    
                                    // Create new RTF with explicit line height
                                    let new_rtf = format!(
                                        "{{\\rtf1\\ansi\\ansicpg1252\\cocoartf1671\\cocoasubrtf600\n\
                                        {{\\fonttbl\\f0\\fnil\\fcharset0 Helvetica;}}\n\
                                        {{\\colortbl;\\red255\\green255\\blue255;\\red255\\green255\\blue255;}}\n\
                                        {{\\*\\expandedcolortbl;;\\csgenericrgb\\c100000\\c100000\\c100000;}}\n\
                                        \\pard\\tx720\\tx1440\\tx2160\\tx2880\\tx3600\\tx4320\\pardirnatural\\qc\\partightenfactor0\n\
                                        \\f0\\fs96 \\sl355\\slmult1 \\cf2 {}}}", content
                                    );
                                    
                                    // Replace the RTF data
                                    text.rtf_data = new_rtf.into_bytes();
                                    
                                    // Set paragraph attributes explicitly
                                    if let Some(attributes) = &mut text.attributes {
                                        if let Some(paragraph_style) = &mut attributes.paragraph_style {
                                            paragraph_style.line_height_multiple = 1.5;
                                            paragraph_style.line_spacing = 0.0;
                                            paragraph_style.maximum_line_height = 0.0;
                                            paragraph_style.minimum_line_height = 0.0;
                                        }
                                        
                                        // Add character spacing (kerning) of 5.0
                                        attributes.kerning = 5.0;
                                    }
                                    
                                    // Add kerning/character spacing to RTF
                                    let rtf_string = String::from_utf8_lossy(&text.rtf_data).to_string();
                                    
                                    // Extract any text content from the RTF
                                    let content_start = rtf_string.find("\\cf2").map(|pos| pos + 4).unwrap_or(0);
                                    let content_end = rtf_string.rfind("}").unwrap_or(rtf_string.len());
                                    let content = if content_start < content_end {
                                        &rtf_string[content_start..content_end]
                                    } else {
                                        ""
                                    };
                                    
                                    // Create new RTF with line height and character spacing
                                    let new_rtf = format!(
                                        "{{\\rtf1\\ansi\\ansicpg1252\\cocoartf1671\\cocoasubrtf600\n\
                                        {{\\fonttbl\\f0\\fnil\\fcharset0 Helvetica;}}\n\
                                        {{\\colortbl;\\red255\\green255\\blue255;\\red255\\green255\\blue255;}}\n\
                                        {{\\*\\expandedcolortbl;;\\csgenericrgb\\c100000\\c100000\\c100000;}}\n\
                                        \\pard\\tx720\\tx1440\\tx2160\\tx2880\\tx3600\\tx4320\\pardirnatural\\qc\\partightenfactor0\n\
                                        \\f0\\fs96 \\sl355\\slmult1 \\expndtw100 \\cf2 {}}}", content
                                    );
                                    
                                    // Replace the RTF data
                                    text.rtf_data = new_rtf.into_bytes();
                                }
                                
                                // Add the rectangle path to each graphics element
                                graphics_element.path = Some(rv_data::graphics::Path {
                                    closed: true,
                                    points: vec![
                                        rv_data::graphics::path::BezierPoint {
                                            point: Some(rv_data::graphics::Point { x: 0.0, y: 0.0 }),
                                            q0: Some(rv_data::graphics::Point { x: 0.0, y: 0.0 }),
                                            q1: Some(rv_data::graphics::Point { x: 0.0, y: 0.0 }),
                                            curved: false,
                                        },
                                        rv_data::graphics::path::BezierPoint {
                                            point: Some(rv_data::graphics::Point { x: 1.0, y: 0.0 }),
                                            q0: Some(rv_data::graphics::Point { x: 1.0, y: 0.0 }),
                                            q1: Some(rv_data::graphics::Point { x: 1.0, y: 0.0 }),
                                            curved: false,
                                        },
                                        rv_data::graphics::path::BezierPoint {
                                            point: Some(rv_data::graphics::Point { x: 1.0, y: 1.0 }),
                                            q0: Some(rv_data::graphics::Point { x: 1.0, y: 1.0 }),
                                            q1: Some(rv_data::graphics::Point { x: 1.0, y: 1.0 }),
                                            curved: false,
                                        },
                                        rv_data::graphics::path::BezierPoint {
                                            point: Some(rv_data::graphics::Point { x: 0.0, y: 1.0 }),
                                            q0: Some(rv_data::graphics::Point { x: 0.0, y: 1.0 }),
                                            q1: Some(rv_data::graphics::Point { x: 0.0, y: 1.0 }),
                                            curved: false,
                                        },
                                    ],
                                    shape: Some(rv_data::graphics::path::Shape {
                                        r#type: rv_data::graphics::path::shape::Type::Rectangle as i32,
                                        additional_data: None,
                                    }),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Save the presentation
    let mut file = File::create("amazing_grace_recreated.pro")?;
    let mut buf = Vec::new();
    rv_presentation.encode(&mut buf)?;
    file.write_all(&buf)?;

    Ok(())
} 