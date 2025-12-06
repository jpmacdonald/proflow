//! ProPresenter data model types.
//!
//! These types represent the full ProPresenter file format structure.
//! Many are not yet used but are defined for future export functionality.

#![allow(dead_code)]

use std::path::PathBuf;
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Represents a ProPresenter presentation
#[derive(Debug, Clone)]
pub struct Presentation {
    /// Name of the presentation
    pub name: String,
    
    /// Path to the presentation file, if it was loaded from disk
    pub path: Option<PathBuf>,
    
    /// Unique identifier for the presentation
    pub uuid: Uuid,
    
    /// Last time the presentation was used
    pub last_used: Option<DateTime<Utc>>,
    
    /// Last time the presentation was modified
    pub last_modified: Option<DateTime<Utc>>,
    
    /// Category of the presentation
    pub category: String,
    
    /// Notes about the presentation
    pub notes: String,
    
    /// CCLI information if this is a song
    pub ccli: Option<CCLIInfo>,
    
    /// Bible reference if this is a scripture
    pub bible_reference: Option<BibleReference>,
    
    /// Cues in the presentation (containing slides as actions)
    pub cues: Vec<Cue>,
    
    /// Cue groups for organizing cues
    pub cue_groups: Vec<CueGroup>,
    
    /// Arrangements of slides (different orderings/groupings)
    pub arrangements: Vec<Arrangement>,
    
    /// Timeline for automated playback
    pub timeline: Option<Timeline>,
    
    /// Application version information
    pub application_info: Option<ApplicationInfo>,
    
    /// Music key if this is a song
    pub music_key: String,
    
    /// Music-specific settings
    pub music: Option<Music>,
    
    /// Slideshow-specific settings
    pub slide_show: Option<SlideShow>,
}

/// CCLI (Christian Copyright Licensing International) information
#[derive(Debug, Clone)]
pub struct CCLIInfo {
    pub author: String,
    pub artist_credits: String,
    pub song_title: String,
    pub publisher: String,
    pub copyright_year: u32,
    pub song_number: u32,
    pub display: bool,
    pub album: String,
}

/// Bible reference information
#[derive(Debug, Clone)]
pub struct BibleReference {
    pub book_index: u32,
    pub book_name: String,
    pub book_key: String,
    pub chapter_range: Range,
    pub verse_range: Range,
    pub translation_name: String,
    pub translation_display_abbreviation: String,
    pub translation_internal_abbreviation: String,
}

/// Represents a numeric range (for chapters/verses)
#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub start: u32,
    pub end: u32,
}

/// Content destination for actions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentDestination {
    Global,
}

/// Layer identification for actions
#[derive(Debug, Clone)]
pub struct LayerIdentification {
    pub uuid: Uuid,
    pub name: String,
}

/// Action types that can be triggered in a timeline
#[derive(Debug, Clone)]
pub enum Action {
    Clear {
        target_layer: i32,
        content_destination: ContentDestination,
    },
    AudienceLook {
        name: String,
        uuid: Uuid,
        parameter_name: String,
    },
    Slide {
        uuid: Uuid,
        name: String,
        slide: Slide,
        delay_time: f64,
        duration: f64,
        enabled: bool,
        layer_identification: Option<LayerIdentification>,
    },
    Media {
        uuid: Uuid,
        name: String,
        source: MediaSource,
        fit: MediaFit,
        opacity: f32,
        volume: f32,
        delay_time: f64,
        duration: f64,
        enabled: bool,
    },
    Macro {
        uuid: Uuid,
        name: String,
        parameter_uuid: Uuid,
        parameter_name: String,
        parent_collection_uuid: Option<Uuid>,
        parent_collection_name: Option<String>,
        delay_time: f64,
        duration: f64,
        enabled: bool,
    },
    // Add other action types as needed
}

/// A cue point in the timeline
#[derive(Debug, Clone)]
pub struct TimelineCue {
    pub trigger_time: f64,
    pub name: String,
    pub uuid: Uuid,
    pub action: Action,
}

/// Timeline for automated presentation playback
#[derive(Debug, Clone)]
pub struct Timeline {
    pub duration: f64,
    pub loop_enabled: bool,
    pub timecode_enabled: bool,
    pub timecode_offset: f64,
    pub cues: Vec<TimelineCue>,
}

/// An arrangement of slides
#[derive(Debug, Clone)]
pub struct Arrangement {
    pub uuid: Uuid,
    pub name: String,
    pub group_identifiers: Vec<Uuid>,
}

/// Represents a single slide in a presentation
#[derive(Debug, Clone)]
pub struct Slide {
    /// Base slide properties
    pub base: BaseSlide,
    
    /// Slide-specific notes
    pub notes: Option<String>,
    
    /// Template guidelines
    pub template_guidelines: Vec<Guideline>,
    
    /// Associated chord chart
    pub chord_chart: Option<Url>,
    
    /// Transition settings
    pub transition: Option<Transition>,
}

/// Base slide properties
#[derive(Debug, Clone)]
pub struct BaseSlide {
    /// Unique identifier
    pub uuid: Uuid,
    
    /// Elements on the slide
    pub elements: Vec<Element>,
    
    /// Build order for elements
    pub element_build_order: Vec<Uuid>,
    
    /// Layout guidelines
    pub guidelines: Vec<Guideline>,
    
    /// Whether to draw background color
    pub draws_background_color: bool,
    
    /// Background color
    pub background_color: Option<Color>,
    
    /// Slide size
    pub size: Size,
}

/// Layout guideline
#[derive(Debug, Clone)]
pub struct Guideline {
    pub position: f64,
    pub orientation: GuidelineOrientation,
}

/// Guideline orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidelineOrientation {
    Horizontal,
    Vertical,
}

/// Represents a color in RGBA format
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl Default for Color {
    fn default() -> Self {
        Self { red: 1.0, green: 1.0, blue: 1.0, alpha: 1.0 }
    }
}

/// Represents an element that can appear on a slide
#[derive(Debug, Clone)]
pub enum Element {
    /// Text element with formatting
    Text(TextElement),
    
    /// Shape element (rectangle, circle, etc)
    Shape {
        shape_type: ShapeType,
        fill_color: Color,
        border_color: Option<Color>,
        border_width: f64,
    },
    
    /// Media element (image, video)
    Media {
        source: MediaSource,
        fit: MediaFit,
        opacity: f32,
        volume: f32,
    },
    
    /// Countdown timer
    Timer {
        format: TimerFormat,
        duration: chrono::Duration,
        text_color: Color,
    },
}

/// Text alignment options
#[derive(Debug, Clone, PartialEq)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
    Justified,
}

/// Shape types
#[derive(Debug, Clone)]
pub enum ShapeType {
    Rectangle,
    Ellipse,
    Triangle,
    Line,
    Custom(String),
}

/// Media source types
#[derive(Debug, Clone, PartialEq)]
pub enum MediaSource {
    File(PathBuf),
    VideoInput {
        input_id: Uuid,
        input_name: String,
    },
    Url(String),
}

/// Media fit options
#[derive(Debug, Clone, PartialEq)]
pub enum MediaFit {
    Scale,
    Stretch,
    Center,
}

/// Timer format options
#[derive(Debug, Clone)]
pub enum TimerFormat {
    ElapsedTime,
    RemainingTime,
    Countdown,
}

/// Represents a size with width and height
#[derive(Debug, Clone, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// Represents a gradient fill
#[derive(Debug, Clone, PartialEq)]
pub struct GradientFill {
    pub angle: f64,
    pub stops: Vec<GradientStop>,
}

/// Represents a single stop in a gradient
#[derive(Debug, Clone, PartialEq)]
pub struct GradientStop {
    pub position: f64,
    pub color: Color,
}

/// Music-specific settings
#[derive(Debug, Clone)]
pub struct Music {
    pub tempo: f64,
    pub time_signature: TimeSignature,
    pub key: String, // e.g., "C", "G#", "Db"
    pub capo: u8,
    pub subdivision: Subdivision,

    pub arrangement_info: Option<ArrangementInfo>, // Add this
    pub rehearsal_mix: Option<MediaSource>,      // Add this
    pub click_track: Option<MediaSource>,        // Add this
    pub chord_chart: Option<Url>,                // Add this
    pub lyrics: String,                         // Add this
    pub sheet_music: Option<Url>,               // Add this
    pub audio_file: Option<MediaSource>,        // Add this
    pub background_audio: Option<MediaSource>,  // Add this
    pub timeline_start: f64,                    // Add this
    pub timeline_end: f64,                      // Add this
}

// ArrangementInfo
#[derive(Debug, Clone)]
pub struct ArrangementInfo {
    pub name: String,
    pub key: String,
    pub tempo: f64,
    pub time_signature: TimeSignature,
    pub notes: String,
}

/// Time signature (e.g., 4/4, 3/4)
#[derive(Debug, Clone)]
pub struct TimeSignature {
    pub beats_per_measure: u8,
    pub beat_unit: u8, // 4 for quarter note, 8 for eighth note, etc.
}

/// Subdivision (e.g., quarter note, eighth note)
#[derive(Debug, Clone)]
pub enum Subdivision {
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

/// Slideshow-specific settings
#[derive(Debug, Clone)]
pub struct SlideShow {
    pub playback_mode: PlaybackMode,
    pub loop_enabled: bool,
    pub transition_duration: f64,

    pub background_color: Option<Color>,
    pub background_image: Option<MediaSource>,
    pub background_video: Option<MediaSource>,
    pub transition: Option<Transition>,
    pub slide_duration: f64,
    pub loop_presentation: bool,
}

/// Playback modes for slideshows
#[derive(Debug, Clone)]
pub enum PlaybackMode {
    Normal,
    Loop,
    Random,
}

/// Application information
#[derive(Debug, Clone)]
pub struct ApplicationInfo {
    pub name: String,
    pub version: String,
    pub build_number: String,
    pub platform: String,
}

/// Represents a URL
#[derive(Debug, Clone)]
pub struct Url {
    pub url: String,
}

/// Represents a transition
#[derive(Debug, Clone)]
pub struct Transition {
    pub transition_type: TransitionType,
    pub duration: f64,
}

/// Transition types
#[derive(Debug, Clone)]
pub enum TransitionType {
    Cut,
    Dissolve,
    Push { direction: Direction },
    Wipe { direction: Direction },
    FadeThroughBlack,
}

/// Direction for transitions
#[derive(Debug, Clone)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Represents a point
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Represents a rectangle
#[derive(Debug, Clone, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

/// Represents paragraph style
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphStyle {
    pub alignment: TextAlignment,
    pub first_line_head_indent: f64,
    pub head_indent: f64,
    pub tail_indent: f64,
    pub line_height_multiple: f64,
    pub maximum_line_height: f64,
    pub minimum_line_height: f64,
    pub line_spacing: f64,
    pub paragraph_spacing: f64,
    pub paragraph_spacing_before: f64,
    pub tab_stops: Vec<TabStop>,
    pub default_tab_interval: f64,
    pub text_list: Option<TextList>,
    pub text_lists: Vec<TextList>,
}

/// Represents a tab stop
#[derive(Debug, Clone, PartialEq)]
pub struct TabStop {
    pub position: f64,
    pub alignment: TabAlignment,
}

/// Tab alignment options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAlignment {
    Left,
    Center,
    Right,
    Decimal,
}

/// Represents a text list
#[derive(Debug, Clone, PartialEq)]
pub struct TextList {
    pub indent_level: u32,
    pub number_type: TextListNumberType,
    pub prefix: String,
    pub suffix: String,
    pub start_index: u32,
    pub indent: f64,
}

/// Text list number types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextListNumberType {
    None,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

/// Represents a font
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub name: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub family: String,
    pub face: String,
}

/// Represents a shadow
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub color: Color,
    pub radius: f64,
    pub offset: Point,
    pub opacity: f32,
    pub angle: f64,
    pub style: ShadowStyle,
    pub enable: bool,
}

/// Shadow style options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowStyle {
    Drop,
}

/// Default implementation for ShadowStyle
impl Default for ShadowStyle {
    fn default() -> Self {
        ShadowStyle::Drop
    }
}

/// Represents a line fill mask
#[derive(Debug, Clone, PartialEq)]
pub struct LineFillMask {
    pub fill_type: LineFillType,
    pub color: Option<Color>,
    pub gradient: Option<GradientFill>,
    pub angle: f64,
    pub spacing: f64,
    pub width: f64,
    pub phase: f64,
}

/// Line fill types
#[derive(Debug, Clone, PartialEq)]
pub enum LineFillType {
    None,
    Color,
    Gradient,
}

/// Text scroller direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Text scroller properties
#[derive(Debug, Clone, PartialEq)]
pub struct TextScroller {
    pub should_scroll: bool,
    pub scroll_rate: f64,
    pub should_repeat: bool,
    pub repeat_distance: f64,
    pub scrolling_direction: ScrollDirection,
    pub starts_off_screen: bool,
    pub fade_left: f64,
    pub fade_right: f64,
}

/// Represents a text element
#[derive(Debug, Clone, PartialEq)]
pub struct TextElement {
    pub content: String,
    pub font: Font,
    pub color: Color,
    pub paragraph_style: ParagraphStyle,
    pub shadow: Option<Shadow>,
    pub mask: Option<LineFillMask>,
    pub bounds: Option<Rect>,
    pub custom_attributes: Vec<CustomAttribute>,
    pub text_scroller: Option<TextScroller>,
}

/// Represents edge insets
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeInsets {
    pub top: f64,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: None,
            uuid: Uuid::new_v4(),
            last_used: None,
            last_modified: None,
            category: String::new(),
            notes: String::new(),
            ccli: None,
            bible_reference: None,
            cues: Vec::new(),
            cue_groups: Vec::new(),
            arrangements: Vec::new(),
            timeline: None,
            application_info: None,
            music_key: String::new(),
            music: None,
            slide_show: None,
        }
    }
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            base: BaseSlide {
                uuid: Uuid::new_v4(),
                elements: Vec::new(),
                element_build_order: Vec::new(),
                guidelines: Vec::new(),
                draws_background_color: true,
                background_color: None,
                size: Size { width: 1920.0, height: 1080.0 },
            },
            notes: None,
            template_guidelines: Vec::new(),
            chord_chart: None,
            transition: None,
        }
    }
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            alignment: TextAlignment::Left,
            first_line_head_indent: 0.0,
            head_indent: 0.0,
            tail_indent: 0.0,
            line_height_multiple: 1.0,
            maximum_line_height: 0.0,
            minimum_line_height: 0.0,
            line_spacing: 0.0,
            paragraph_spacing: 0.0,
            paragraph_spacing_before: 0.0,
            tab_stops: Vec::new(),
            default_tab_interval: 0.0,
            text_list: None,
            text_lists: Vec::new(),
        }
    }
}

// ===== Fill and Effect Types =====

/// Fill properties
#[derive(Debug, Clone)]
pub struct Fill {
    pub enabled: bool,
    pub fill_type: Option<FillType>,
}

/// Fill type options
#[derive(Debug, Clone)]
pub enum FillType {
    Color(Color),
    Gradient(GradientFill),
    Image(ImageFill),
}

/// Image fill properties
#[derive(Debug, Clone, PartialEq)]
pub struct ImageFill {
    pub image_data: Vec<u8>,
    pub image_type: String,
    pub scale: f64,
    pub offset: Point,
    pub rotation: f64,
}

/// Stroke properties
#[derive(Debug, Clone)]
pub struct Stroke {
    pub style: StrokeStyle,
    pub width: f64,
    pub color: Option<Color>,
    pub pattern: Vec<f64>,
    pub enabled: bool,
}

/// Stroke style options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeStyle {
    SolidLine,
    DashedLine,
    DottedLine,
}

/// Feather effect properties
#[derive(Debug, Clone)]
pub struct Feather {
    pub style: FeatherStyle,
    pub radius: f64,
    pub enabled: bool,
}

/// Feather style options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatherStyle {
    Inside,
    Outside,
    Both,
}

// ===== Mask Types =====

/// Mask properties
#[derive(Debug, Clone)]
pub enum Mask {
    TextLine(LineFillMask),
    Shape(ShapeMask),
}

/// Shape mask properties
#[derive(Debug, Clone)]
pub struct ShapeMask {
    pub shape: Shape,
    pub invert: bool,
}

/// Mask style options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskStyle {
    FullWidth,
    TextWidth,
    MaxLineWidth,
}

// ===== Shape Types =====

/// Shape properties
#[derive(Debug, Clone)]
pub struct Shape {
    pub shape_type: ShapeType,
    pub additional_data: Option<String>,
}

// ===== Chord Pro =====

/// Chord pro settings
#[derive(Debug, Clone)]
pub struct ChordPro {
    pub enabled: bool,
    pub notation: ChordNotation,
    pub color: Option<Color>,
}

/// Chord notation options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordNotation {
    Chords,
    Nashville,
    Roman,
}

// ===== Scale Behavior =====

/// Scale behavior options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleBehavior {
    None,
    FitWidth,
    FitHeight,
    Fill,
    Stretch,
}

// ===== Text Transform =====

/// Text transform options
#[derive(Debug, Clone)]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
    Custom(String),
}

// ===== Flip Mode =====

/// Flip mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlipMode {
    None,
    Horizontal,
    Vertical,
    Both,
}

/// Text attributes
#[derive(Debug, Clone, PartialEq)]
pub struct TextAttributes {
    pub font: Option<Font>,
    pub capitalization: Capitalization,
    pub underline_style: Option<UnderlineStyle>,
    pub underline_color: Option<Color>,
    pub paragraph_style: Option<ParagraphStyle>,
    pub kerning: f64,
    pub superscript: i32,
    pub strikethrough_style: Option<StrikethroughStyle>,
    pub strikethrough_color: Option<Color>,
    pub stroke_width: f64,
    pub stroke_color: Option<Color>,
    pub custom_attributes: Vec<CustomAttribute>,
    pub background_color: Option<Color>,
    pub ligature_style: LigatureStyle,
    pub fill: Option<TextFill>,
}

/// Text fill options
#[derive(Debug, Clone, PartialEq)]
pub enum TextFill {
    TextSolidFill(Color),
    TextGradientFill(GradientFill),
    CutOutFill(CutOutFill),
    MediaFill(MediaFill),
    BackgroundEffect(BackgroundEffect),
}

/// Cut out fill properties
#[derive(Debug, Clone, PartialEq)]
pub struct CutOutFill {
    pub enabled: bool,
}

/// Media fill properties
#[derive(Debug, Clone, PartialEq)]
pub struct MediaFill {
    pub source: MediaSource,
    pub fit: MediaFit,
    pub opacity: f32,
}

impl Default for MediaFill {
    fn default() -> Self {
        Self {
            source: MediaSource::File(PathBuf::new()),
            fit: MediaFit::Scale,
            opacity: 1.0,
        }
    }
}

/// Background effect properties
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundEffect {
    pub effect_type: BackgroundEffectType,
    pub intensity: f64,
}

/// Background effect types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundEffectType {
    None,
    Blur,
    Darken,
    Lighten,
}

/// Text capitalization options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capitalization {
    None,
    AllCaps,
    SmallCaps,
    TitleCase,
    StartCase,
}

/// Underline style options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineStyle {
    None,
    Single,
    Double,
    Thick,
}

/// Strikethrough style options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrikethroughStyle {
    None,
    Single,
    Double,
}

/// Ligature style options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LigatureStyle {
    Default,
    None,
}

/// Custom text attribute
#[derive(Debug, Clone, PartialEq)]
pub struct CustomAttribute {
    pub range: Range,
    pub attribute: CustomAttributeType,
}

/// Custom attribute types
#[derive(Debug, Clone, PartialEq)]
pub enum CustomAttributeType {
    Capitalization(Capitalization),
    OriginalFontSize(f64),
    FontScaleFactor(f64),
    TextGradientFill(GradientFill),
    ShouldPreserveForegroundColor(bool),
    Chord(String),
    CutOutFill(CutOutFill),
    MediaFill(MediaFill),
    BackgroundEffect(BackgroundEffect),
    CharacterSizeMode(CharacterSizeMode),
}

/// Character size mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterSizeMode {
    Normal,
    ScaledByDocumentHeight,
    ScaledByDocumentWidth,
}

// Add builder methods for Timeline
impl Timeline {
    pub fn new() -> Self {
        Self {
            duration: 0.0,
            loop_enabled: false,
            timecode_enabled: false,
            timecode_offset: 0.0,
            cues: Vec::new(),
        }
    }

    pub fn with_duration(mut self, duration: f64) -> Self {
        self.duration = duration;
        self
    }

    pub fn with_loop(mut self, loop_enabled: bool) -> Self {
        self.loop_enabled = loop_enabled;
        self
    }

    pub fn add_cue(&mut self, cue: TimelineCue) {
        self.cues.push(cue);
    }
}

// Add builder methods for TimelineCue
impl TimelineCue {
    pub fn new(trigger_time: f64, name: impl Into<String>, action: Action) -> Self {
        Self {
            trigger_time,
            name: name.into(),
            uuid: Uuid::new_v4(),
            action,
        }
    }

    pub fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = uuid;
        self
    }
}

/// Represents a cue in a presentation
#[derive(Debug, Clone)]
pub struct Cue {
    /// Unique identifier for the cue
    pub uuid: Uuid,
    
    /// Name of the cue
    pub name: String,
    
    /// Actions contained in this cue
    pub actions: Vec<Action>,
    
    /// Whether the cue is enabled
    pub enabled: bool,
    
    /// Hot key associated with this cue
    pub hot_key: Option<HotKey>,
    
    /// Completion behavior settings
    pub completion_target_type: CompletionTargetType,
    pub completion_target_uuid: Option<Uuid>,
    pub completion_action_type: CompletionActionType,
    pub completion_action_uuid: Option<Uuid>,
    pub completion_time: f64,
}

/// Completion target type for cues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionTargetType {
    None,
    Next,
    Random,
    Cue,
    First,
}

/// Completion action type for cues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionActionType {
    First,
    Last,
    AfterAction,
    AfterTime,
}

/// Hot key definition for cues
#[derive(Debug, Clone)]
pub struct HotKey {
    pub key_code: u32,
    pub modifiers: u32,
    pub control_identifier: String,
}

/// Represents a cue group in a presentation
#[derive(Debug, Clone)]
pub struct CueGroup {
    /// The group details
    pub group: Group,
    
    /// Identifiers of cues within this group
    pub cue_identifiers: Vec<Uuid>,
}

/// Represents a group for organizing cues
#[derive(Debug, Clone)]
pub struct Group {
    /// Unique identifier
    pub uuid: Uuid,
    
    /// Name of the group
    pub name: String,
    
    /// Color of the group (used in UI)
    pub color: Color,
    
    /// Hot key associated with this group
    pub hot_key: Option<HotKey>,
    
    /// Application-specific identifier
    pub application_group_identifier: String,
} 