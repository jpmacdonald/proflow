//! Editor service trait and text manipulation utilities.
//!
//! This module provides abstractions for text editing operations, allowing
//! the editor logic to be tested independently of the UI.

/// Result of an editor action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// The action was handled and state was modified.
    Modified,
    /// The action was handled but no state changed.
    NoChange,
    /// The action requires exiting editor mode.
    Exit,
    /// The action was not handled by the editor.
    Unhandled,
}

/// Trait for text editing operations.
///
/// This trait abstracts the core editing functionality, allowing for
/// different implementations (e.g., single-line, multi-line, with undo).
pub trait Editor {
    /// Get the current content as lines.
    fn content(&self) -> &[String];

    /// Get mutable access to content.
    fn content_mut(&mut self) -> &mut Vec<String>;

    /// Get the current cursor position (line, column).
    fn cursor_position(&self) -> (usize, usize);

    /// Set the cursor position.
    fn set_cursor(&mut self, line: usize, column: usize);

    /// Insert a character at the current cursor position.
    fn insert_char(&mut self, c: char);

    /// Insert a string at the current cursor position.
    fn insert_str(&mut self, s: &str);

    /// Delete the character before the cursor (backspace).
    fn delete_backward(&mut self);

    /// Delete the character at the cursor (delete).
    fn delete_forward(&mut self);

    /// Check if there is any non-whitespace content.
    fn has_content(&self) -> bool {
        self.content().iter().any(|line| !line.trim().is_empty())
    }

    /// Get the total number of lines.
    fn line_count(&self) -> usize {
        self.content().len()
    }
}

/// Selection range in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Start line (0-indexed).
    pub start_line: usize,
    /// Start column (0-indexed).
    pub start_col: usize,
    /// End line (0-indexed).
    pub end_line: usize,
    /// End column (0-indexed).
    pub end_col: usize,
}

impl Selection {
    /// Create a new selection.
    #[must_use]
    pub const fn new(start_line: usize, start_col: usize, end_line: usize, end_col: usize) -> Self {
        Self {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    /// Normalize the selection so start comes before end.
    #[must_use]
    pub const fn normalized(&self) -> Self {
        if self.start_line > self.end_line
            || (self.start_line == self.end_line && self.start_col > self.end_col)
        {
            Self {
                start_line: self.end_line,
                start_col: self.end_col,
                end_line: self.start_line,
                end_col: self.start_col,
            }
        } else {
            *self
        }
    }

    /// Check if the selection is empty (zero length).
    pub const fn is_empty(&self) -> bool {
        self.start_line == self.end_line && self.start_col == self.end_col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_normalized() {
        // Already normalized
        let sel = Selection::new(0, 5, 1, 10);
        let norm = sel.normalized();
        assert_eq!(norm.start_line, 0);
        assert_eq!(norm.end_line, 1);

        // Needs normalization (end before start)
        let sel = Selection::new(1, 10, 0, 5);
        let norm = sel.normalized();
        assert_eq!(norm.start_line, 0);
        assert_eq!(norm.start_col, 5);
        assert_eq!(norm.end_line, 1);
        assert_eq!(norm.end_col, 10);
    }

    #[test]
    fn test_selection_is_empty() {
        let empty = Selection::new(5, 10, 5, 10);
        assert!(empty.is_empty());

        let not_empty = Selection::new(5, 10, 5, 11);
        assert!(!not_empty.is_empty());
    }
}
