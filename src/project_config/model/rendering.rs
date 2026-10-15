//! Cue roles, display bindings, and speaker colors in the JSON contract.

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

/// Named binding between a semantic cue role and `ProPresenter` assets.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CueRoleConfig {
    /// Theme slide name used for cues in this role.
    pub slide: String,
    /// Semantic text fields mapped to exact named graphics elements.
    ///
    /// An empty map enables the conventional `body` field only when the theme
    /// slide has exactly one meaningful text destination. Explicit mappings
    /// allow multi-field templates without relying on element order or UUIDs.
    #[serde(default)]
    pub text_slots: BTreeMap<String, String>,
    /// Macro placed on the first cue entering this role.
    pub enter_macro: Option<String>,
    /// Alternate entry macro when a cue's first visible speaker is the leader.
    pub leader_enter_macro: Option<String>,
    /// Editor colors used to preserve leader/audience text runs.
    pub speaker_colors: Option<SpeakerColorConfig>,
}

/// Editor colors for semantic liturgical speakers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpeakerColorConfig {
    /// Leader/liturgist text color in the projector-oriented editor theme.
    pub leader: RgbColor,
    /// Congregational text color in the projector-oriented editor theme.
    pub audience: RgbColor,
}

/// Checked six-digit RGB color serialized as `#RRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(with = "String")]
pub struct RgbColor((u8, u8, u8));

impl RgbColor {
    /// Build one RGB color from explicit channel values.
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self((red, green, blue))
    }

    /// Return the checked native RGB components.
    pub const fn components(self) -> (u8, u8, u8) {
        self.0
    }
}

impl Serialize for RgbColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!(
            "#{:02X}{:02X}{:02X}",
            self.0 .0, self.0 .1, self.0 .2
        ))
    }
}

impl<'de> Deserialize<'de> for RgbColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let hex = value
            .strip_prefix('#')
            .ok_or_else(|| serde::de::Error::custom("RGB color must use #RRGGBB notation"))?;
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(serde::de::Error::custom(
                "RGB color must use #RRGGBB notation",
            ));
        }
        let component = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&hex[range], 16).map_err(serde::de::Error::custom)
        };
        Ok(Self::new(
            component(0..2)?,
            component(2..4)?,
            component(4..6)?,
        ))
    }
}

/// Binding between presentation regions and named cue roles.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DisplayBindingConfig {
    /// Every generated cue uses one role.
    Single {
        /// Named cue role.
        role: String,
    },
    /// The title cue and content cues use distinct roles.
    Split {
        /// Cue role for the title cue.
        title: String,
        /// Cue role for content cues.
        content: String,
    },
}
