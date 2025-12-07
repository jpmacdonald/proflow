//! Template-based slide generation.
//!
//! Loads donor presentation templates and injects text while preserving styling.

use std::collections::HashMap;
use std::path::PathBuf;
use prost::Message;

use super::generated::rv_data;
use super::rtf::text_to_rtf_bytes;

/// Slide types that can use templates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateType {
    Scripture,
    Song,
    Info,
}

impl TemplateType {
    /// Get the template filename for this type
    pub fn filename(&self) -> &'static str {
        match self {
            Self::Scripture => "__template_scripture__.pro",
            Self::Song => "__template_song__.pro",
            Self::Info => "__template_info__.pro",
        }
    }
    
    /// All template types
    pub fn all() -> &'static [TemplateType] {
        &[Self::Scripture, Self::Song, Self::Info]
    }
}

/// Cached template presentations
pub struct TemplateCache {
    templates: HashMap<TemplateType, rv_data::Presentation>,
    search_paths: Vec<PathBuf>,
}

impl TemplateCache {
    /// Create a new template cache with search paths
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        Self {
            templates: HashMap::new(),
            search_paths,
        }
    }
    
    /// Add a search path
    pub fn add_search_path(&mut self, path: PathBuf) {
        if !self.search_paths.contains(&path) {
            self.search_paths.push(path);
        }
    }
    
    /// Try to find and load a template
    fn find_template(&self, template_type: TemplateType) -> Option<rv_data::Presentation> {
        let filename = template_type.filename();
        
        for search_path in &self.search_paths {
            let path = search_path.join(filename);
            if path.exists() {
                if let Ok(data) = std::fs::read(&path) {
                    if let Ok(presentation) = rv_data::Presentation::decode(data.as_slice()) {
                        return Some(presentation);
                    }
                }
            }
        }
        None
    }
    
    /// Get a template, loading it if necessary
    pub fn get(&mut self, template_type: TemplateType) -> Option<&rv_data::Presentation> {
        if !self.templates.contains_key(&template_type) {
            if let Some(template) = self.find_template(template_type) {
                self.templates.insert(template_type, template);
            }
        }
        self.templates.get(&template_type)
    }
    
    /// Check if a template is available
    pub fn has_template(&mut self, template_type: TemplateType) -> bool {
        self.get(template_type).is_some()
    }
    
    /// Load templates from raw bytes (e.g., from embedded playlist)
    pub fn load_from_bytes(&mut self, template_type: TemplateType, data: &[u8]) -> bool {
        if let Ok(presentation) = rv_data::Presentation::decode(data) {
            self.templates.insert(template_type, presentation);
            true
        } else {
            false
        }
    }
}

/// Extract the first slide's text element from a template presentation
pub fn extract_template_slide(presentation: &rv_data::Presentation) -> Option<rv_data::PresentationSlide> {
    // Navigate: cues -> actions -> slide
    for cue in &presentation.cues {
        for action in &cue.actions {
            if let Some(rv_data::action::ActionTypeData::Slide(slide_type)) = &action.action_type_data {
                if let Some(rv_data::action::slide_type::Slide::Presentation(slide)) = &slide_type.slide {
                    return Some(slide.clone());
                }
            }
        }
    }
    None
}

/// Clone a template slide and replace its text content
pub fn clone_slide_with_text(template_slide: &rv_data::PresentationSlide, new_text: &str) -> rv_data::PresentationSlide {
    let mut slide = template_slide.clone();
    
    // Navigate: base_slide -> elements -> element (graphics::Element) -> text
    if let Some(ref mut base_slide) = slide.base_slide {
        for slide_element in &mut base_slide.elements {
            if let Some(ref mut graphics_element) = slide_element.element {
                if let Some(ref mut text) = graphics_element.text {
                    // Generate new RTF with proper superscript handling
                    text.rtf_data = text_to_rtf_bytes(new_text);
                }
            }
        }
        // Give base slide a new UUID
        base_slide.uuid = Some(rv_data::Uuid { string: uuid::Uuid::new_v4().to_string() });
    }
    
    slide
}

/// Build a complete presentation from a template and content lines
/// 
/// Each line in `content` becomes a separate slide, cloned from the template.
pub fn build_presentation_from_template(
    name: &str,
    template: &rv_data::Presentation,
    content: &[String],
) -> Option<rv_data::Presentation> {
    let template_slide = extract_template_slide(template)?;
    
    let mut presentation = template.clone();
    presentation.name = name.to_string();
    presentation.uuid = Some(rv_data::Uuid { string: uuid::Uuid::new_v4().to_string() });
    
    // Clear existing cues and groups
    presentation.cues.clear();
    presentation.cue_groups.clear();
    presentation.arrangements.clear();
    
    // Create a cue for each content line (stanza)
    let mut cue_uuids = Vec::new();
    
    for (i, text) in content.iter().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        
        let slide = clone_slide_with_text(&template_slide, text);
        let cue_uuid = uuid::Uuid::new_v4();
        let action_uuid = uuid::Uuid::new_v4();
        
        let cue = rv_data::Cue {
            uuid: Some(rv_data::Uuid { string: cue_uuid.to_string() }),
            name: format!("Slide {}", i + 1),
            actions: vec![rv_data::Action {
                uuid: Some(rv_data::Uuid { string: action_uuid.to_string() }),
                name: format!("Slide {}", i + 1),
                label: None,
                delay_time: 0.0,
                old_type: None,
                is_enabled: true,
                layer_identification: None,
                duration: 0.0,
                r#type: rv_data::action::ActionType::PresentationSlide as i32,
                action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                    rv_data::action::SlideType {
                        slide: Some(rv_data::action::slide_type::Slide::Presentation(slide)),
                    }
                )),
            }],
            completion_target_type: rv_data::cue::CompletionTargetType::None as i32,
            completion_target_uuid: None,
            completion_action_type: rv_data::cue::CompletionActionType::First as i32,
            completion_action_uuid: None,
            trigger_time: Some(rv_data::cue::TimecodeTime { time: 0.0 }),
            hot_key: None,
            pending_imports: Vec::new(),
            is_enabled: true,
            completion_time: 0.0,
        };
        
        cue_uuids.push(cue_uuid);
        presentation.cues.push(cue);
    }
    
    // Create a single group containing all cues
    if !cue_uuids.is_empty() {
        let group_uuid = uuid::Uuid::new_v4();
        let group = rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid { string: group_uuid.to_string() }),
                name: "Default".to_string(),
                color: Some(rv_data::Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 1.0,
                    alpha: 1.0,
                }),
                hot_key: None,
                application_group_identifier: Some(rv_data::Uuid { string: uuid::Uuid::new_v4().to_string() }),
                application_group_name: String::new(),
            }),
            cue_identifiers: cue_uuids.iter()
                .map(|u| rv_data::Uuid { string: u.to_string() })
                .collect(),
        };
        presentation.cue_groups.push(group);
        
        // Create default arrangement
        let arrangement = rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid { string: uuid::Uuid::new_v4().to_string() }),
            name: "Default".to_string(),
            group_identifiers: vec![rv_data::Uuid { string: group_uuid.to_string() }],
        };
        presentation.arrangements.push(arrangement);
        presentation.selected_arrangement = presentation.arrangements.first()
            .and_then(|a| a.uuid.clone());
    }
    
    Some(presentation)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn get_template_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("data");
        path.push("templates");
        path
    }
    
    #[test]
    fn test_template_cache_load() {
        let mut cache = TemplateCache::new(vec![get_template_path()]);
        
        // Should find scripture template
        assert!(cache.has_template(TemplateType::Scripture));
        
        let template = cache.get(TemplateType::Scripture).unwrap();
        assert!(!template.cues.is_empty());
    }
    
    #[test]
    fn test_extract_template_slide() {
        let mut cache = TemplateCache::new(vec![get_template_path()]);
        let template = cache.get(TemplateType::Scripture).unwrap();
        
        let slide = extract_template_slide(template);
        assert!(slide.is_some());
    }
    
    #[test]
    fn test_build_from_template() {
        let mut cache = TemplateCache::new(vec![get_template_path()]);
        let template = cache.get(TemplateType::Scripture).unwrap().clone();
        
        let content = vec![
            "¹⁵The wilderness and dry land shall be glad,".to_string(),
            "¹⁶the desert shall rejoice and blossom;".to_string(),
        ];
        
        let presentation = build_presentation_from_template("Test Scripture", &template, &content);
        assert!(presentation.is_some());
        
        let pres = presentation.unwrap();
        assert_eq!(pres.name, "Test Scripture");
        assert_eq!(pres.cues.len(), 2);
    }
}

