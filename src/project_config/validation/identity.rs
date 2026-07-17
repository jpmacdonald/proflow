//! Shared validation for exact identifiers, filenames, and generated names.

use super::ConfigValidationIssue;
use std::collections::HashSet;

/// Return the runtime identity of one valid presentation filename.
///
/// `ProPresenter`'s optional `.pro` suffix and filename case are not semantic.
/// Invalid wire values have no identity and are reported by the ordinary
/// filename validators instead.
pub(super) fn canonical_presentation_filename(value: &str) -> Option<String> {
    if value.trim().is_empty()
        || value.trim() != value
        || value.chars().any(char::is_control)
        || value.contains(['/', '\\'])
        || matches!(value, "." | "..")
    {
        return None;
    }

    let normalized = value.to_lowercase();
    let stem = normalized
        .strip_suffix(".pro")
        .unwrap_or(&normalized)
        .trim();
    (!stem.is_empty()).then(|| stem.to_string())
}

pub(super) fn validate_library_filename(
    value: &str,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    validate_presentation_filename(value, path, "library_file", issues);
}

pub(super) fn validate_presentation_filename(
    value: &str,
    path: &str,
    label: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if !validate_exact_identity(value, path, label, issues) {
        return;
    }
    let lower = value.to_ascii_lowercase();
    let stem = lower.strip_suffix(".pro").unwrap_or(&lower);
    if matches!(value, "." | "..") || stem.is_empty() {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: format!("{label} must name a presentation"),
        });
    } else if value.contains(['/', '\\']) {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: format!("{label} must be a filename, not a path"),
        });
    }
}

pub(super) fn validate_name_template(
    value: &str,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if !validate_exact_identity(value, path, "name_template", issues) {
        return;
    }
    if value.contains(['/', '\\']) {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: "name_template must produce a filename, not a path".to_string(),
        });
    }
    if matches!(value, "." | "..") || value.eq_ignore_ascii_case(".pro") {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: "name_template must produce a presentation name".to_string(),
        });
    }

    let mut remainder = value;
    loop {
        let Some(open) = remainder.find('{') else {
            if remainder.contains('}') {
                issues.push(ConfigValidationIssue {
                    path: path.to_string(),
                    message: "name_template contains an unmatched '}'".to_string(),
                });
            }
            break;
        };
        if remainder[..open].contains('}') {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: "name_template contains an unmatched '}'".to_string(),
            });
            break;
        }
        let after_open = &remainder[open + 1..];
        let Some(close) = after_open.find('}') else {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: "name_template contains an unmatched '{'".to_string(),
            });
            break;
        };
        let placeholder = &after_open[..close];
        if !matches!(placeholder, "speaker" | "first_name" | "title") {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: format!("name_template contains unknown placeholder '{{{placeholder}}}'"),
            });
        }
        remainder = &after_open[close + 1..];
    }
}

pub(super) fn validate_map_keys<'a>(
    keys: impl Iterator<Item = &'a String>,
    path: &str,
    label: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let mut canonical_keys = HashSet::new();
    for key in keys {
        validate_exact_identity(key, path, label, issues);
        if !canonical_keys.insert(key.to_ascii_lowercase()) {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: format!("ambiguous {label} '{key}' differs only by case"),
            });
        }
    }
}

pub(super) fn validate_identity_values(
    values: &[String],
    path: &str,
    label: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let mut canonical_values = HashSet::new();
    for (index, value) in values.iter().enumerate() {
        validate_exact_identity(value, &format!("{path}[{index}]"), label, issues);
        if !canonical_values.insert(value.to_ascii_lowercase()) {
            issues.push(ConfigValidationIssue {
                path: format!("{path}[{index}]"),
                message: format!("duplicate {label} '{value}'"),
            });
        }
    }
}

pub(super) fn validate_exact_identity(
    value: &str,
    path: &str,
    label: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) -> bool {
    let message = if value.trim().is_empty() {
        Some(format!("{label} must not be blank"))
    } else if value.trim() != value {
        Some(format!("{label} must be unpadded"))
    } else if value.chars().any(char::is_control) {
        Some(format!("{label} must not contain control characters"))
    } else {
        None
    };
    let Some(message) = message else {
        return true;
    };
    issues.push(ConfigValidationIssue {
        path: path.to_string(),
        message,
    });
    false
}
