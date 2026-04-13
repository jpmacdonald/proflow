# Config V2 Schema

## Purpose

Define the current v2 project config for ProFlow that:

- drives runtime behavior
- drives the available MCP-discoverable profiles
- remains church-specific without hardcoding church-specific assumptions into Rust
- gives the LLM a stable schema to inspect, explain, and patch

This replaces the older loose split between:

- `presentation_types`
- `item_types`
- `library_files`
- `multi_expand`
- `skip_items`
- `staff`
- `service_types`
- `service_overrides`

## Design Decisions

1. Profiles are config-defined, not hardcoded.
2. `edited` is removed and replaced by explicit policy fields.
3. Ordered rules are better than scattered top-level maps.
4. Service grouping is generic.
5. MCP and any internal tooling both reflect the configured project, not generic church assumptions.

## Top-Level Shape

Current config root:

```json
{
  "version": 2,
  "metadata": {},
  "defaults": {},
  "service_groups": {},
  "profiles": {},
  "presentation_types": {},
  "item_rules": [],
  "people": {},
  "overrides": []
}
```

## Rust Model Sketch

```rust
pub struct ProjectConfigV2 {
    pub version: u16,
    pub metadata: ProjectMetadata,
    pub defaults: ProjectDefaults,
    pub service_groups: HashMap<String, ServiceGroupConfig>,
    pub profiles: HashMap<String, ProfileConfig>,
    pub presentation_types: HashMap<String, PresentationTypeConfig>,
    pub item_rules: Vec<ItemRuleConfig>,
    pub people: HashMap<String, PersonConfig>,
    pub overrides: Vec<OverrideRuleConfig>,
}
```

Supporting enums:

```rust
pub enum ReviewPolicy {
    Ask,
    Fail,
    Skip,
}

pub enum ContentSourceKind {
    Static,
    Description,
    Scripture,
    Song,
}

pub enum OutputStrategy {
    Skip,
    UseExisting,
    EditInPlace,
    GenerateNew,
    NeedsReview,
}

pub enum ItemKind {
    Song,
    Scripture,
    Liturgy,
    Nametag,
    Announcement,
    Graphic,
    Other,
}
```

## `metadata`

Optional project metadata. This is descriptive, not operational.

```json
{
  "name": "Village Presbyterian Church",
  "timezone": "America/New_York",
  "notes": "Traditional service workflow"
}
```

Suggested shape:

```rust
pub struct ProjectMetadata {
    pub name: Option<String>,
    pub timezone: Option<String>,
    pub notes: Option<String>,
}
```

## `defaults`

Project-wide defaults for runtime behavior.

```json
{
  "theme": "VPC Theme",
  "days_ahead": 30,
  "review_policy": "ask",
  "plan_sort": "ascending_date"
}
```

Suggested shape:

```rust
pub struct ProjectDefaults {
    pub theme: Option<String>,
    pub days_ahead: Option<i64>,
    pub review_policy: Option<ReviewPolicy>,
    pub plan_sort: Option<PlanSort>,
}
```

## `service_groups`

Named reusable sets of service types.

These are intentionally generic. A church may define:

- `weekly_primary`
- `wednesday`
- `seasonal`
- `special_events`

Example:

```json
{
  "weekly_primary": {
    "service_types": ["9:00am contemporary", "10:30am traditional"]
  },
  "seasonal": {
    "service_types": ["Christmas Eve", "Ash Wednesday", "Maundy Thursday"]
  }
}
```

Suggested shape:

```rust
pub struct ServiceGroupConfig {
    pub service_types: Vec<String>,
}
```

## `profiles`

Profiles are optional named build presets.

If a project defines no profiles, MCP should still support explicit selectors like:

- service type
- plan id
- days ahead

Profiles let a project express its normal workflows declaratively.

Example:

```json
{
  "weekly": {
    "description": "Normal weekly prep for primary Sunday services",
    "service_groups": ["weekly_primary"],
    "days_ahead": 14,
    "review_policy": "ask"
  },
  "seasonal": {
    "description": "Seasonal and holiday services",
    "service_groups": ["seasonal"],
    "days_ahead": 60,
    "review_policy": "ask"
  }
}
```

Suggested shape:

```rust
pub struct ProfileConfig {
    pub description: Option<String>,
    pub service_groups: Vec<String>,
    pub service_types: Vec<String>,
    pub days_ahead: Option<i64>,
    pub review_policy: Option<ReviewPolicy>,
}
```

Rules:

- `service_groups` references entries in `service_groups`
- `service_types` may be used directly for one-off configs
- the effective service type set is the union of both
- profile values override `defaults`

## `presentation_types`

This is the main policy layer.

Each presentation type must declare behavior explicitly.

Example:

```json
{
  "scripture": {
    "kind": "scripture",
    "content_source": "scripture",
    "output_strategy": "generate_new",
    "template": "Scripture (Projectors)",
    "title_template": "Information (Projectors)",
    "background": "default",
    "macro": "Scripture/Prayer",
    "arrangement": null,
    "description": "Scripture slides generated from Bible data"
  },
  "liturgical_weekly": {
    "kind": "liturgy",
    "content_source": "description",
    "output_strategy": "edit_in_place",
    "template": "Scripture (Projectors) (Responsive)",
    "background": "default",
    "macro": "Scripture/Prayer",
    "arrangement": null,
    "description": "Weekly liturgy regenerated from description into an existing file"
  }
}
```

Suggested shape:

```rust
pub struct PresentationTypeConfig {
    pub kind: ItemKind,
    pub content_source: ContentSourceKind,
    pub output_strategy: OutputStrategy,
    pub template: Option<String>,
    pub title_template: Option<String>,
    pub background: Option<String>,
    pub macro_name: Option<String>,
    pub arrangement: Option<String>,
    pub description: Option<String>,
}
```

Notes:

- `title_template` is only relevant for cases like scripture with distinct title/content slides.
- `output_strategy` replaces the current overloaded `edited`.
- `template` is the default template used by this type, unless a rule or override changes it.

## `item_rules`

This is the most important change.

Replace separate prefix maps and expansion maps with an ordered array of rules.

Ordered rules are easier to explain, validate, and patch.

Example:

```json
[
  {
    "id": "skip_benediction",
    "match": {
      "title_prefix": ["benediction"]
    },
    "action": {
      "kind": "skip",
      "reason": "handled live"
    }
  },
  {
    "id": "welcome_bundle",
    "match": {
      "title_prefix": ["welcome"]
    },
    "expand": [
      {
        "use_type": "person_nametag",
        "speaker": "resolved"
      },
      {
        "use_type": "announcements"
      }
    ]
  },
  {
    "id": "call_to_worship",
    "match": {
      "title_prefix": ["call to worship"]
    },
    "use_type": "liturgical_weekly",
    "target": {
      "library_file": "Call to Worship.pro"
    }
  }
]
```

Suggested shape:

```rust
pub struct ItemRuleConfig {
    pub id: String,
    pub match_spec: MatchSpec,
    pub use_type: Option<String>,
    pub action: Option<RuleAction>,
    pub expand: Vec<ExpansionStep>,
    pub target: Option<TargetSpec>,
    pub notes: Option<String>,
}
```

### `match`

Minimal useful matching fields:

```rust
pub struct MatchSpec {
    pub title_prefix: Vec<String>,
    pub title_contains: Vec<String>,
    pub category: Option<String>,
    pub has_scripture_ref: Option<bool>,
    pub service_type: Vec<String>,
}
```

This is enough to replace the current config without making the rule language too large.

### `action`

This is for explicit non-type actions like skip or review.

```rust
pub enum RuleAction {
    Skip { reason: String },
    Review { reason: String },
}
```

### `expand`

Use this for multi-output rules like:

- welcome -> speaker nametag + announcements
- call to worship -> speaker nametag + liturgy

```rust
pub struct ExpansionStep {
    pub use_type: String,
    pub speaker: Option<SpeakerSource>,
    pub target: Option<TargetSpec>,
}
```

## `target`

Targets define where the result should come from or be written to.

This is how we make in-place edits explicit.

Example:

```json
{
  "library_file": "Call to Worship.pro"
}
```

Suggested shape:

```rust
pub struct TargetSpec {
    pub library_file: Option<String>,
    pub name_template: Option<String>,
}
```

Interpretation:

- `use_existing` + `library_file` means reference the known file
- `edit_in_place` + `library_file` means overwrite that known file
- `generate_new` + `name_template` means render a new file with computed name

This is safer and more legible than inferring target behavior from multiple maps.

## `people`

This replaces `staff`.

Example:

```json
{
  "Hope": {
    "last": "Lee",
    "role": "pastor",
    "nametag": "Hope Nametag"
  },
  "Robert": {
    "last": "Austell",
    "role": "pastor",
    "nametag": "Robert Nametag"
  }
}
```

Suggested shape:

```rust
pub struct PersonConfig {
    pub last: Option<String>,
    pub role: Option<String>,
    pub nametag: Option<String>,
}
```

This should help both speaker resolution and LLM config suggestions.

## `overrides`

Keep overrides explicit and structured.

Use these sparingly for service-group-specific changes like arrangement or background differences.

Example:

```json
[
  {
    "when": {
      "service_group": "weekly_primary",
      "presentation_type": "song"
    },
    "arrangement": "Traditional"
  }
]
```

Suggested shape:

```rust
pub struct OverrideRuleConfig {
    pub when: OverrideWhen,
    pub arrangement: Option<String>,
    pub background: Option<String>,
    pub template: Option<String>,
}
```

## Example Full Config

For a compact starter version of this shape, see [examples/starter-config.json](/Users/jimmy/Documents/Projects/proflow/examples/starter-config.json).

```json
{
  "version": 2,
  "metadata": {
    "name": "Village Presbyterian Church",
    "timezone": "America/New_York"
  },
  "defaults": {
    "theme": "VPC Theme",
    "days_ahead": 30,
    "review_policy": "ask"
  },
  "service_groups": {
    "weekly_primary": {
      "service_types": ["9:00am contemporary", "10:30am traditional"]
    },
    "seasonal": {
      "service_types": ["Christmas Eve"]
    }
  },
  "profiles": {
    "weekly": {
      "description": "Primary weekly service prep",
      "service_groups": ["weekly_primary"],
      "days_ahead": 14,
      "review_policy": "ask"
    },
    "seasonal": {
      "description": "Seasonal service prep",
      "service_groups": ["seasonal"],
      "days_ahead": 60,
      "review_policy": "ask"
    }
  },
  "presentation_types": {
    "person_nametag": {
      "kind": "nametag",
      "content_source": "static",
      "output_strategy": "use_existing",
      "template": "Name Tag",
      "macro": "Name Tag/Title",
      "description": "Static speaker nametag"
    },
    "announcements": {
      "kind": "graphic",
      "content_source": "static",
      "output_strategy": "use_existing",
      "macro": "Graphics",
      "description": "Static announcements slides"
    },
    "liturgical_weekly": {
      "kind": "liturgy",
      "content_source": "description",
      "output_strategy": "edit_in_place",
      "template": "Scripture (Projectors) (Responsive)",
      "background": "default",
      "macro": "Scripture/Prayer",
      "description": "Weekly liturgy regenerated from PCO description"
    },
    "scripture": {
      "kind": "scripture",
      "content_source": "scripture",
      "output_strategy": "generate_new",
      "template": "Scripture (Projectors)",
      "title_template": "Information (Projectors)",
      "background": "default",
      "macro": "Scripture/Prayer",
      "description": "Scripture generated from Bible data"
    },
    "song": {
      "kind": "song",
      "content_source": "song",
      "output_strategy": "use_existing",
      "template": "Lyrics (Projectors)",
      "macro": "Song",
      "arrangement": "Default",
      "description": "Song from existing library"
    }
  },
  "item_rules": [
    {
      "id": "skip_benediction",
      "match": {
        "title_prefix": ["benediction"]
      },
      "action": {
        "kind": "skip",
        "reason": "handled live"
      }
    },
    {
      "id": "welcome_bundle",
      "match": {
        "title_prefix": ["welcome"]
      },
      "expand": [
        {
          "use_type": "person_nametag",
          "speaker": "resolved"
        },
        {
          "use_type": "announcements",
          "target": {
            "library_file": "Announcements.pro"
          }
        }
      ]
    },
    {
      "id": "call_to_worship",
      "match": {
        "title_prefix": ["call to worship"]
      },
      "expand": [
        {
          "use_type": "person_nametag",
          "speaker": "resolved"
        },
        {
          "use_type": "liturgical_weekly",
          "target": {
            "library_file": "Call to Worship.pro"
          }
        }
      ]
    }
  ],
  "people": {
    "Hope": {
      "last": "Lee",
      "role": "pastor",
      "nametag": "Hope Nametag"
    },
    "Robert": {
      "last": "Austell",
      "role": "pastor",
      "nametag": "Robert Nametag"
    }
  },
  "overrides": []
}
```

## MCP Implications

MCP should expose config-aware tools:

- `get_context`
- `list_profiles`
- `validate_config`
- `explain_rule_match`
- `find_unmapped_items`
- `suggest_config_patch`

The LLM should help improve config, but the runtime build should always go through this schema.

## Migration From Current Config

Current field -> v2 mapping:

- `theme` -> `defaults.theme`
- `presentation_types.*.edited` -> `presentation_types.*.content_source` + `presentation_types.*.output_strategy`
- `item_types` -> `item_rules[].match` + `item_rules[].use_type`
- `library_files` -> `item_rules[].target.library_file`
- `multi_expand` -> `item_rules[].expand`
- `skip_items` -> `item_rules[].action = skip`
- `staff` -> `people`
- `service_types` -> `service_groups`
- `service_overrides` -> `overrides`

## Recommended Implementation Subset

Implement v2 in two passes.

Pass 1:

- `version`
- `defaults`
- `service_groups`
- `profiles`
- `presentation_types`
- `item_rules`
- `people`

Pass 2:

- `overrides`
- advanced match fields
- richer target policies
- config patch suggestion workflow

This keeps the first implementation scoped and still useful.
