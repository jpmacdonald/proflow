//! Service plan preview — analyzes a PCO plan and proposes playlist entries.
//!
//! Uses a declarative type system from `data/proflow.config.json` to classify
//! each PCO item, resolve library files, and produce a structured preview.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::description_parser::{self, ParsedContent};
use crate::planning_center::types::Item;
use crate::utils::file_index::FileIndex;

// ---------------------------------------------------------------------------
// Config loaded from data/proflow.config.json
// ---------------------------------------------------------------------------

/// Root config structure.
#[derive(Debug, Default, Deserialize)]
pub struct ServiceConfig {
    /// `ProPresenter` theme name to load slide templates from.
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub presentation_types: HashMap<String, PresentationType>,
    #[serde(default)]
    pub item_types: HashMap<String, String>,
    #[serde(default)]
    pub library_files: HashMap<String, String>,
    #[serde(default)]
    pub multi_expand: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub skip_items: Vec<String>,
    #[serde(default)]
    pub staff: HashMap<String, StaffEntry>,
    #[serde(default)]
    pub service_overrides: HashMap<String, HashMap<String, TypeOverride>>,
    #[serde(default)]
    pub nametag_pattern: Option<String>,
    #[serde(default)]
    pub service_types: Option<ServiceTypes>,
}

/// Service type groupings.
#[derive(Debug, Default, Deserialize)]
pub struct ServiceTypes {
    #[serde(default)]
    pub primary: Vec<String>,
    #[serde(default)]
    pub seasonal: Vec<String>,
}

/// Declares the behavior of a presentation type.
#[derive(Debug, Default, Deserialize)]
pub struct PresentationType {
    pub template: Option<String>,
    #[serde(default)]
    pub edited: bool,
    pub background: Option<String>,
    pub arrangement: Option<String>,
    /// `ProPresenter` macro to trigger on the first slide.
    #[serde(rename = "macro")]
    pub macro_name: Option<String>,
    #[serde(default)]
    pub description: String,
}

/// Per-service override for a presentation type's properties.
#[derive(Debug, Default, Deserialize)]
pub struct TypeOverride {
    pub arrangement: Option<String>,
    pub background: Option<String>,
}

/// Staff member entry.
#[derive(Debug, Deserialize)]
pub struct StaffEntry {
    #[serde(default)]
    pub last: String,
    #[serde(default)]
    pub role: String,
}

// ---------------------------------------------------------------------------
// Preview output
// ---------------------------------------------------------------------------

/// Individual scripture reference within a multi-reference item.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptureRefInfo {
    /// Parsed reference string (e.g., "Isaiah 35:1-6").
    pub reference: String,
    /// Bible version (e.g., "`NRSVue`").
    pub version: String,
}

/// Status of a proposed playlist entry.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    /// Existing library file, no changes needed.
    Used,
    /// New file generated from scratch (scripture, etc.).
    Created,
    /// Library file whose content is refreshed from this week's description.
    Edited,
    /// Not included in the playlist.
    #[default]
    Skipped,
    /// Needs user confirmation.
    Uncertain,
}

/// A single row in the preview table.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PreviewEntry {
    pub position: usize,
    pub pco_title: String,
    pub playlist_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub status: PreviewStatus,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_content: Option<ParsedContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripture_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bible_version: Option<String>,
    /// Individual scripture references for multi-reference items.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub scripture_refs: Option<Vec<ScriptureRefInfo>>,
    /// Theme slide name to use for generation (from `presentation_types.template`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_name: Option<String>,
    /// `ProPresenter` macro to trigger on the first slide.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macro_name: Option<String>,
}

/// Full preview result.
#[derive(Debug, Serialize)]
pub struct PreviewResult {
    pub plan_title: String,
    pub service_name: String,
    pub date: String,
    pub entries: Vec<PreviewEntry>,
    pub summary: PreviewSummary,
}

/// Summary counts for the preview.
#[derive(Debug, Serialize)]
pub struct PreviewSummary {
    pub used_count: usize,
    pub created_count: usize,
    pub edited_count: usize,
    pub skip_count: usize,
    pub uncertain_count: usize,
    pub total_playlist_items: usize,
}

// ---------------------------------------------------------------------------
// Preview builder
// ---------------------------------------------------------------------------

/// Resolve the effective arrangement for a type, considering service overrides.
fn resolve_arrangement(
    ptype: &PresentationType,
    type_key: &str,
    service_name: Option<&str>,
    mappings: &ServiceConfig,
) -> Option<String> {
    // Service override takes precedence
    if let Some(svc) = service_name {
        let svc_lower = svc.to_lowercase();
        if let Some(overrides) = mappings.service_overrides.get(&svc_lower) {
            if let Some(ovr) = overrides.get(type_key) {
                if ovr.arrangement.is_some() {
                    return ovr.arrangement.clone();
                }
            }
        }
    }
    ptype.arrangement.clone()
}

/// Resolve the effective background for a type, considering service overrides.
fn resolve_background(
    ptype: &PresentationType,
    type_key: &str,
    service_name: Option<&str>,
    mappings: &ServiceConfig,
) -> Option<String> {
    if let Some(svc) = service_name {
        let svc_lower = svc.to_lowercase();
        if let Some(overrides) = mappings.service_overrides.get(&svc_lower) {
            if let Some(ovr) = overrides.get(type_key) {
                if ovr.background.is_some() {
                    return ovr.background.clone();
                }
            }
        }
    }
    ptype.background.clone()
}

/// Build a preview of the proposed playlist for a set of PCO items.
#[allow(clippy::too_many_lines)]
pub fn build_preview(
    items: &[Item],
    mappings: &ServiceConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> Vec<PreviewEntry> {
    let mut entries = Vec::new();
    let mut nametag_seen: HashSet<String> = HashSet::new();

    for item in items {
        let title_lower = item.title.to_lowercase();
        let speaker = extract_speaker(&item.title);

        // 1. Check skip list
        if should_skip(&title_lower, mappings) {
            entries.push(PreviewEntry {
                position: item.position,
                pco_title: item.title.clone(),
                reason: skip_reason(&title_lower),
                ..Default::default()
            });
            continue;
        }

        // 2. Check multi-item expansion (welcome, call to worship)
        if let Some(expansion) = find_expansion(&title_lower, mappings) {
            process_expansion(
                &expansion,
                item,
                speaker.as_deref(),
                mappings,
                &mut entries,
                &mut nametag_seen,
                file_index,
                service_name,
            );
            continue;
        }

        // 3. Resolve item type from config
        let resolved_type = resolve_item_type(&title_lower, mappings);

        // 4. Songs — special handling (search library, skip if not found)
        if item.category == crate::planning_center::types::Category::Song {
            let song_title = item
                .song
                .as_ref()
                .map_or(item.title.as_str(), |s| s.title.as_str());
            let found = search_index_strict(file_index, song_title)
                .or_else(|| search_index_strict(file_index, &strip_title_prefix(&item.title)));

            let song_type = mappings.presentation_types.get("song");
            let song_arrangement = song_type.map_or_else(
                || None,
                |pt| resolve_arrangement(pt, "song", service_name, mappings),
            );

            entries.push(PreviewEntry {
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: song_title.to_string(),
                file_path: found.clone(),
                status: if found.is_some() {
                    PreviewStatus::Used
                } else {
                    PreviewStatus::Skipped
                },
                reason: if found.is_some() {
                    "Library match".to_string()
                } else {
                    "Not in library — skip".to_string()
                },
                item_type: Some("song".to_string()),
                arrangement: song_arrangement,
                template_name: mappings.presentation_types.get("song").and_then(|pt| pt.template.clone()),
                macro_name: mappings.presentation_types.get("song").and_then(|pt| pt.macro_name.clone()),
                ..Default::default()
            });
            continue;
        }

        // 5. Scripture — single entry even for multi-ref items like "Isa 1:1; Luke 2:3"
        if item.scripture.is_some() || has_scripture_ref(&item.title) {
            maybe_insert_nametag(
                &mut entries,
                &mut nametag_seen,
                speaker.as_deref(),
                item,
                file_index,
            );

            let scripture_bg = mappings
                .presentation_types
                .get("scripture")
                .and_then(|pt| resolve_background(pt, "scripture", service_name, mappings));

            let ref_parts = split_scripture_refs(&item.title);
            let version = detect_version(&item.title);

            if ref_parts.len() > 1 {
                // Multi-reference: one entry with scripture_refs holding individual refs
                let ref_infos: Vec<ScriptureRefInfo> = ref_parts
                    .iter()
                    .filter_map(|part| {
                        let v = detect_version(part);
                        crate::bible::parse_scripture_ref(part).map(|r| {
                            let ref_str = r.end_verse.map_or_else(
                                || format!("{} {}:{}", r.book, r.chapter, r.start_verse),
                                |end| format!("{} {}:{}-{end}", r.book, r.chapter, r.start_verse),
                            );
                            ScriptureRefInfo {
                                reference: ref_str,
                                version: v.to_string(),
                            }
                        })
                    })
                    .collect();

                let combined_name = ref_infos
                    .iter()
                    .map(|r| {
                        r.reference
                            .replace(':', "v")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let playlist_name = format!("{combined_name} {version}");

                entries.push(PreviewEntry {
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name,
                    status: PreviewStatus::Created,
                    reason: format!(
                        "Generate combined scripture slides ({} refs, {version})",
                        ref_infos.len()
                    ),
                    item_type: Some("scripture".to_string()),
                    background: scripture_bg.clone(),
                    bible_version: Some(version.to_string()),
                    scripture_refs: Some(ref_infos),
                    template_name: mappings.presentation_types.get("scripture").and_then(|pt| pt.template.clone()),
                    macro_name: mappings.presentation_types.get("scripture").and_then(|pt| pt.macro_name.clone()),
                    ..Default::default()
                });
            } else {
                // Single reference
                let ref_part = &ref_parts[0];
                let scripture_ref_str = crate::bible::parse_scripture_ref(ref_part)
                    .map(|r| {
                        r.end_verse.map_or_else(
                            || format!("{} {}:{}", r.book, r.chapter, r.start_verse),
                            |end| format!("{} {}:{}-{end}", r.book, r.chapter, r.start_verse),
                        )
                    });

                entries.push(PreviewEntry {
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: scripture_name(ref_part, version),
                    status: PreviewStatus::Created,
                    reason: format!("Generate scripture slides ({version})"),
                    item_type: Some("scripture".to_string()),
                    background: scripture_bg,
                    scripture_reference: scripture_ref_str,
                    bible_version: Some(version.to_string()),
                    template_name: mappings.presentation_types.get("scripture").and_then(|pt| pt.template.clone()),
                    macro_name: mappings.presentation_types.get("scripture").and_then(|pt| pt.macro_name.clone()),
                    ..Default::default()
                });
            }
            continue;
        }

        // 6. Known item type — behavior comes from the type declaration
        if let Some((type_key, ptype)) = &resolved_type {
            let library_file = find_library_file(&title_lower, mappings);
            let found = library_file
                .as_ref()
                .and_then(|name| search_index(file_index, name.trim_end_matches(".pro")));

            maybe_insert_nametag(
                &mut entries,
                &mut nametag_seen,
                speaker.as_deref(),
                item,
                file_index,
            );

            let (status, reason) = match (&found, ptype.edited) {
                (Some(_), true) => (
                    PreviewStatus::Edited,
                    "Content updated from description".to_string(),
                ),
                (Some(_), false) => (PreviewStatus::Used, "Library match".to_string()),
                (None, true) if item.description.is_some() => (
                    PreviewStatus::Edited,
                    "Generate from description content".to_string(),
                ),
                (None, _) => (PreviewStatus::Skipped, "No library match".to_string()),
            };

            // Parse description for edited types
            let parsed = if ptype.edited {
                item.description
                    .as_deref()
                    .and_then(|desc| description_parser::parse_description(desc, &item.title, type_key))
            } else {
                None
            };

            let bg = resolve_background(ptype, type_key, service_name, mappings);
            let arr = resolve_arrangement(ptype, type_key, service_name, mappings);

            let playlist_name = found.as_ref().map_or_else(
                || strip_speaker(&item.title),
                |path| {
                    std::path::Path::new(path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&item.title)
                        .to_string()
                },
            );

            entries.push(PreviewEntry {
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name,
                file_path: found,
                status,
                reason,
                item_type: Some(type_key.clone()),
                parsed_content: parsed,
                background: bg,
                arrangement: arr,
                template_name: ptype.template.clone(),
                macro_name: ptype.macro_name.clone(),
                ..Default::default()
            });
            continue;
        }

        // 7. Fallback — search library by title variants
        let stripped = strip_title_prefix(&item.title);
        let clean = strip_speaker(&item.title);
        let found = search_index(file_index, &clean)
            .or_else(|| search_index(file_index, &stripped))
            .or_else(|| search_index(file_index, &item.title));

        maybe_insert_nametag(
            &mut entries,
            &mut nametag_seen,
            speaker.as_deref(),
            item,
            file_index,
        );

        if let Some(path) = found {
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&clean)
                .to_string();
            entries.push(PreviewEntry {
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: name,
                file_path: Some(path),
                status: PreviewStatus::Used,
                reason: "Library match".to_string(),
                ..Default::default()
            });
        } else {
            entries.push(PreviewEntry {
                position: item.position,
                pco_title: item.title.clone(),
                reason: "No library match".to_string(),
                ..Default::default()
            });
        }
    }

    // Dedup consecutive entries pointing to the same file
    entries.dedup_by(|b, a| a.file_path.is_some() && a.file_path == b.file_path);

    entries
}

// ---------------------------------------------------------------------------
// Type resolution
// ---------------------------------------------------------------------------

fn resolve_item_type<'a>(
    title_lower: &str,
    mappings: &'a ServiceConfig,
) -> Option<(String, &'a PresentationType)> {
    // Exact match first, then prefix match
    let type_key = mappings
        .item_types
        .get(title_lower)
        .cloned()
        .or_else(|| {
            mappings
                .item_types
                .iter()
                .find(|(k, _)| title_lower.starts_with(k.as_str()))
                .map(|(_, v)| v.clone())
        })?;

    let ptype = mappings.presentation_types.get(&type_key)?;
    Some((type_key, ptype))
}

fn find_library_file(title_lower: &str, mappings: &ServiceConfig) -> Option<String> {
    mappings
        .library_files
        .get(title_lower)
        .cloned()
        .or_else(|| {
            mappings
                .library_files
                .iter()
                .find(|(k, _)| title_lower.starts_with(k.as_str()))
                .map(|(_, v)| v.clone())
        })
}

// ---------------------------------------------------------------------------
// Expansion processing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn process_expansion(
    expansion: &[String],
    item: &Item,
    speaker: Option<&str>,
    mappings: &ServiceConfig,
    entries: &mut Vec<PreviewEntry>,
    nametag_seen: &mut HashSet<String>,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) {
    let title_lower = item.title.to_lowercase();
    let resolved_type = resolve_item_type(&title_lower, mappings);

    for template in expansion {
        const SPEAKER_PLACEHOLDER: &str = "{speaker}";
        if template.contains(SPEAKER_PLACEHOLDER) {
            if let Some(name) = speaker {
                let first = first_name(name);
                if nametag_seen.contains(&first) {
                    continue;
                }
                nametag_seen.insert(first.clone());
                let nametag_name = format!("{first} Nametag");
                let found = search_index(file_index, &nametag_name);
                entries.push(PreviewEntry {
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: nametag_name.clone(),
                    file_path: found.clone(),
                    status: if found.is_some() {
                        PreviewStatus::Used
                    } else {
                        PreviewStatus::Uncertain
                    },
                    reason: found.map_or_else(
                        || format!("{nametag_name} — not found"),
                        |_| format!("Nametag for {first}"),
                    ),
                    item_type: Some("person_nametag".to_string()),
                    template_name: mappings.presentation_types.get("person_nametag").and_then(|pt| pt.template.clone()),
                    macro_name: mappings.presentation_types.get("person_nametag").and_then(|pt| pt.macro_name.clone()),
                    ..Default::default()
                });
            }
        } else if template == "_generate" {
            let is_edited = resolved_type
                .as_ref()
                .is_some_and(|(_, pt)| pt.edited);

            let type_key_str = resolved_type.as_ref().map(|(k, _)| k.as_str());
            let parsed = if is_edited {
                item.description.as_deref().and_then(|desc| {
                    type_key_str.and_then(|tk| {
                        description_parser::parse_description(desc, &item.title, tk)
                    })
                })
            } else {
                None
            };

            let (bg, arr) = resolved_type.as_ref().map_or((None, None), |(k, pt)| {
                (
                    resolve_background(pt, k, service_name, mappings),
                    resolve_arrangement(pt, k, service_name, mappings),
                )
            });

            entries.push(PreviewEntry {
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                status: if is_edited {
                    PreviewStatus::Edited
                } else {
                    PreviewStatus::Created
                },
                reason: "Generate from description content".to_string(),
                item_type: resolved_type.as_ref().map(|(k, _)| k.clone()),
                parsed_content: parsed,
                background: bg,
                arrangement: arr,
                template_name: resolved_type.as_ref().and_then(|(_, pt)| pt.template.clone()),
                macro_name: resolved_type.as_ref().and_then(|(_, pt)| pt.macro_name.clone()),
                ..Default::default()
            });
        } else {
            let search_name = template.trim_end_matches(".pro");
            let found = search_index(file_index, search_name);
            entries.push(PreviewEntry {
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: search_name.to_string(),
                file_path: found.clone(),
                status: if found.is_some() {
                    PreviewStatus::Used
                } else {
                    PreviewStatus::Uncertain
                },
                reason: found.map_or_else(
                    || format!("{search_name} — not found"),
                    |_| "Library match".to_string(),
                ),
                ..Default::default()
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn should_skip(title_lower: &str, mappings: &ServiceConfig) -> bool {
    mappings
        .skip_items
        .iter()
        .any(|skip| title_lower.starts_with(skip))
}

fn skip_reason(title_lower: &str) -> String {
    if title_lower.starts_with("sermon") {
        "Sermon — skip (added day-of)".to_string()
    } else if title_lower.starts_with("benediction") {
        "Benediction — skip".to_string()
    } else {
        "Skip".to_string()
    }
}

fn find_expansion(title_lower: &str, mappings: &ServiceConfig) -> Option<Vec<String>> {
    mappings
        .multi_expand
        .iter()
        .find(|(k, _)| title_lower.starts_with(k.as_str()))
        .map(|(_, v)| v.clone())
}

fn extract_speaker(title: &str) -> Option<String> {
    let start = title.rfind('(')?;
    let end = title.rfind(')')?;
    (end > start + 1).then(|| title[start + 1..end].trim().to_string())
}

fn first_name(name: &str) -> String {
    name.split_whitespace()
        .next()
        .unwrap_or(name)
        .to_string()
}

fn strip_speaker(title: &str) -> String {
    title
        .rfind('(')
        .map_or_else(|| title.to_string(), |i| title[..i].trim().to_string())
}

fn strip_title_prefix(title: &str) -> String {
    const PREFIXES: &[&str] = &[
        "Organ Prelude:",
        "Organ Postlude:",
        "Offertory:",
        "Youth Choir:",
        "Scripture:",
        "Scripture -",
        "Scripture Reading:",
        "Moment for Mission:",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = title.strip_prefix(prefix) {
            return strip_speaker(rest.trim());
        }
    }
    strip_speaker(title)
}

/// Split a title with multiple scripture references (separated by `;`) into
/// individual reference strings. Preserves version and speaker info on each.
fn split_scripture_refs(title: &str) -> Vec<String> {
    // Strip common prefixes
    let stripped = title
        .trim_start_matches("Scripture Reading:")
        .trim_start_matches("Scripture Reading -")
        .trim_start_matches("Scripture:")
        .trim_start_matches("Scripture -")
        .trim();

    // Remove speaker parenthetical for splitting, re-detect version
    let no_speaker = stripped
        .rfind('(')
        .map_or(stripped, |i| stripped[..i].trim());

    // Detect version from the full title (applies to all refs)
    let version_suffix = detect_version(title);

    let parts: Vec<&str> = no_speaker.split(';').collect();
    if parts.len() <= 1 {
        // Single reference — return the original title for full parsing
        return vec![title.to_string()];
    }

    parts
        .iter()
        .map(|part| {
            let trimmed = part.trim();
            // Strip inline version markers so we can re-append a consistent one
            let clean = trimmed
                .trim_end_matches("NRSVue")
                .trim_end_matches("NRSV")
                .trim_end_matches("NKJV")
                .trim_end_matches("NIV")
                .trim_end_matches("NLT")
                .trim_end_matches("NASB")
                .trim_end_matches("KJV")
                .trim();
            format!("{clean} {version_suffix}")
        })
        .filter(|s| crate::bible::parse_scripture_ref(s).is_some())
        .collect()
}

fn has_scripture_ref(title: &str) -> bool {
    crate::bible::parse_scripture_ref(title).is_some()
}

fn detect_version(title: &str) -> &str {
    let upper = title.to_uppercase();
    for (needle, version) in [
        ("NLT", "NLT"),
        ("NRSVUE", "NRSVue"),
        ("NRSV", "NRSV"),
        ("NKJV", "NKJV"),
        ("NIV", "NIV"),
        ("NASB", "NASB"),
        ("KJV", "KJV"),
    ] {
        if upper.contains(needle) {
            return version;
        }
    }
    "NRSVue"
}

fn scripture_name(title: &str, version: &str) -> String {
    crate::bible::parse_scripture_ref(title).map_or_else(
        || strip_speaker(title),
        |r| {
            let ref_str = r.end_verse.map_or_else(
                || format!("{} {}:{}", r.book, r.chapter, r.start_verse),
                |end| format!("{} {}:{}-{end}", r.book, r.chapter, r.start_verse),
            );
            format!("{ref_str} {version}")
        },
    )
}

/// Standard library search — accepts fuzzy matches with word overlap.
fn search_index(index: Option<&FileIndex>, query: &str) -> Option<String> {
    let idx = index?;
    let matches = idx.find_matches(query, 1);
    let entry = matches.first()?;

    let name_lower = entry.file_name.to_lowercase();
    let query_lower = query.to_lowercase();

    // Exact or containment
    if name_lower == query_lower
        || name_lower.contains(&query_lower)
        || query_lower.contains(&name_lower)
    {
        return Some(entry.full_path.to_string_lossy().to_string());
    }

    // Word overlap
    let query_words: HashSet<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();
    let name_words: HashSet<&str> = name_lower
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();
    let overlap = query_words.intersection(&name_words).count();
    if overlap >= 2 || (overlap == 1 && query_words.len() == 1) {
        return Some(entry.full_path.to_string_lossy().to_string());
    }

    None
}

/// Strict library search for songs — requires containment match, not just
/// word overlap. Prevents false positives like "Have Thine Own Way" matching
/// "We Have Come at Christ's Own Bidding".
fn search_index_strict(index: Option<&FileIndex>, query: &str) -> Option<String> {
    let idx = index?;
    let matches = idx.find_matches(query, 3);

    let query_lower = query.to_lowercase();

    for entry in &matches {
        let name_lower = entry.file_name.to_lowercase();
        if name_lower == query_lower
            || name_lower.contains(&query_lower)
            || query_lower.contains(&name_lower)
        {
            return Some(entry.full_path.to_string_lossy().to_string());
        }
    }

    None
}

fn maybe_insert_nametag(
    entries: &mut Vec<PreviewEntry>,
    nametag_seen: &mut HashSet<String>,
    speaker: Option<&str>,
    item: &Item,
    file_index: Option<&FileIndex>,
) {
    let Some(name) = speaker else { return };
    let first = first_name(name);
    if nametag_seen.contains(&first) {
        return;
    }
    nametag_seen.insert(first.clone());
    let nametag_name = format!("{first} Nametag");
    let found = search_index(file_index, &nametag_name);
    entries.push(PreviewEntry {
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: nametag_name.clone(),
        file_path: found.clone(),
        status: if found.is_some() {
            PreviewStatus::Used
        } else {
            PreviewStatus::Uncertain
        },
        reason: found.map_or_else(
            || format!("{nametag_name} — not found"),
            |_| format!("Nametag for {first}"),
        ),
        item_type: Some("person_nametag".to_string()),
        ..Default::default()
    });
}
