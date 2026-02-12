//! UUID generation for `ProPresenter` files.

#![allow(dead_code)]

use uuid::Uuid as SystemUuid;

/// Generate a new UUID in the format needed by `ProPresenter`
pub fn generate_uuid() -> String {
    let uuid = SystemUuid::new_v4();
    uuid.to_string()
}

/// Convert a string to a UUID or generate a new one if the input is invalid
pub fn string_to_uuid_or_generate(input: Option<&str>) -> String {
    match input {
        Some(s) if !s.is_empty() => {
            SystemUuid::parse_str(s).map_or_else(|_| generate_uuid(), |uuid| uuid.to_string())
        },
        _ => generate_uuid(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_uuid() {
        let uuid = generate_uuid();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn test_string_to_uuid_or_generate_valid() {
        let valid_uuid = "550e8400-e29b-41d4-a716-446655440000";
        let result = string_to_uuid_or_generate(Some(valid_uuid));
        assert_eq!(result, valid_uuid);
    }

    #[test]
    fn test_string_to_uuid_or_generate_invalid() {
        let invalid_uuid = "not-a-uuid";
        let result = string_to_uuid_or_generate(Some(invalid_uuid));
        assert_ne!(result, invalid_uuid);
        assert_eq!(result.len(), 36);
    }

    #[test]
    fn test_string_to_uuid_or_generate_empty() {
        let result = string_to_uuid_or_generate(Some(""));
        assert_eq!(result.len(), 36);
    }

    #[test]
    fn test_string_to_uuid_or_generate_none() {
        let result = string_to_uuid_or_generate(None);
        assert_eq!(result.len(), 36);
    }
} 