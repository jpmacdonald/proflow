//! Editor state and text manipulation logic.
//!
//! Contains `EditorState` for the slide content editor, along with all cursor,
//! selection, clipboard, text editing, stanza analysis, and verse marker operations.

use arboard::Clipboard;
use ratatui::style::Color;

/// Default wrap column width for text wrapping.
pub const DEFAULT_WRAP_COLUMN: usize = 80;

/// Minimum wrap column width allowed.
pub const MIN_WRAP_COLUMN: usize = 10;

/// Default viewport height in lines.
const DEFAULT_VIEWPORT_HEIGHT: usize = 20;

/// Persistent and transient state for the slide content editor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EditorState {
    /// Lines of text content being edited.
    pub content: Vec<String>,
    /// Horizontal cursor position (column index).
    #[serde(default)]
    pub cursor_x: usize,
    /// Vertical cursor position (line index).
    #[serde(default)]
    pub cursor_y: usize,
    /// Number of lines scrolled past the top of the viewport.
    #[serde(default)]
    pub scroll_offset: usize,
    /// Column at which text wraps for slide splitting.
    #[serde(default = "default_wrap_column")]
    pub wrap_column: usize,
    /// Whether wrap column auto-adjusts to viewport width.
    #[serde(default = "default_wrap_auto")]
    pub wrap_auto: bool,
    /// Last known viewport width for auto-wrap calculation.
    #[serde(skip)]
    pub last_viewport_width: Option<usize>,
    /// Buffer for the `:command` being typed.
    #[serde(skip)]
    pub command_buffer: String,
    /// Whether the editor is in `:command` entry mode.
    #[serde(skip)]
    pub is_command_mode: bool,
    /// Visible line count for scroll calculations.
    #[serde(skip, default = "default_viewport_height")]
    pub viewport_height: usize,
    /// Whether a text selection is currently active.
    #[serde(skip)]
    pub selection_active: bool,
    /// Column where the current selection began.
    #[serde(skip)]
    pub selection_start_x: usize,
    /// Line where the current selection began.
    #[serde(skip)]
    pub selection_start_y: usize,
}

const fn default_wrap_column() -> usize {
    DEFAULT_WRAP_COLUMN
}
const fn default_wrap_auto() -> bool {
    true
}
const fn default_viewport_height() -> usize {
    DEFAULT_VIEWPORT_HEIGHT
}

/// A labeled song-section marker (e.g., Verse, Chorus) with its shorthand command.
#[derive(Debug, Clone)]
pub struct VerseGroup {
    /// Full display name (e.g., "Verse 1").
    pub name: String,
    /// Short command string to insert this marker (e.g., "v1").
    pub command: String,
    /// Color used when rendering this marker in the UI.
    pub color: Color,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            content: vec![String::new(), String::new()],
            cursor_x: 0,
            cursor_y: 0,
            scroll_offset: 0,
            wrap_column: default_wrap_column(),
            wrap_auto: default_wrap_auto(),
            last_viewport_width: None,
            command_buffer: String::new(),
            is_command_mode: false,
            viewport_height: DEFAULT_VIEWPORT_HEIGHT,
            selection_active: false,
            selection_start_x: 0,
            selection_start_y: 0,
        }
    }
}

// --- Cursor movement ---

impl EditorState {
    /// Moves the cursor and manages selection state based on whether Shift is held.
    pub const fn handle_cursor_movement(&mut self, new_y: usize, new_x: usize, is_shift_pressed: bool) {
        if is_shift_pressed {
            if !self.selection_active {
                self.selection_active = true;
                self.selection_start_x = self.cursor_x;
                self.selection_start_y = self.cursor_y;
            }
        } else {
            self.selection_active = false;
        }

        self.cursor_y = new_y;
        self.cursor_x = new_x;
    }

    /// Move cursor one position left, wrapping to previous line end if at column 0.
    pub fn handle_left_key(&mut self, is_shift_pressed: bool) {
        if self.cursor_x > 0 {
            self.handle_cursor_movement(self.cursor_y, self.cursor_x - 1, is_shift_pressed);
        } else if self.cursor_y > 0 {
            let new_y = self.cursor_y - 1;
            let new_x = self.content.get(new_y).map_or(0, String::len);
            self.handle_cursor_movement(new_y, new_x, is_shift_pressed);
        }
    }

    /// Move cursor one position right, wrapping to next line start if at line end.
    pub fn handle_right_key(&mut self, is_shift_pressed: bool) {
        let current_line_len = self.content.get(self.cursor_y).map_or(0, String::len);

        if self.cursor_x < current_line_len {
            self.handle_cursor_movement(self.cursor_y, self.cursor_x + 1, is_shift_pressed);
        } else if self.cursor_y < self.content.len() - 1 {
            self.handle_cursor_movement(self.cursor_y + 1, 0, is_shift_pressed);
        }
    }

    /// Move cursor one line up, clamping horizontal position to line length.
    pub fn handle_up_key(&mut self, is_shift_pressed: bool) {
        if self.cursor_y > 0 {
            let new_y = self.cursor_y - 1;
            let new_x = self
                .content
                .get(new_y)
                .map_or(0, |line| self.cursor_x.min(line.len()));
            self.handle_cursor_movement(new_y, new_x, is_shift_pressed);
        }
    }

    /// Move cursor one line down, clamping horizontal position to line length.
    pub fn handle_down_key(&mut self, is_shift_pressed: bool) {
        if self.cursor_y < self.content.len() - 1 {
            let new_y = self.cursor_y + 1;
            let new_x = self
                .content
                .get(new_y)
                .map_or(0, |line| self.cursor_x.min(line.len()));
            self.handle_cursor_movement(new_y, new_x, is_shift_pressed);
        }
    }

    /// Clamp cursor to valid content positions.
    pub fn clamp_cursor(&mut self) {
        if self.content.is_empty() {
            self.content.push(String::new());
        }
        self.cursor_y = self.cursor_y.min(self.content.len().saturating_sub(1));
        let line_len = self.content.get(self.cursor_y).map_or(0, String::len);
        self.cursor_x = self.cursor_x.min(line_len);
    }
}

// --- Text editing ---

impl EditorState {
    /// Insert a character at the current cursor position and advance the cursor.
    pub fn insert_char(&mut self, c: char) {
        if self.cursor_y >= self.content.len() {
            self.content.push(String::new());
        }
        let line = &mut self.content[self.cursor_y];
        if self.cursor_x > line.len() {
            line.push_str(&" ".repeat(self.cursor_x - line.len()));
        }
        line.insert(self.cursor_x, c);
        self.cursor_x += 1;
    }

    /// Insert a line at the given position, or append if beyond bounds.
    pub fn insert_or_append_at(&mut self, pos: usize, content: String) {
        if pos < self.content.len() {
            self.content.insert(pos, content);
        } else {
            self.content.push(content);
        }
    }

    /// Ensure there is always exactly one trailing empty line.
    pub fn ensure_empty_line_at_end(&mut self) {
        if self.content.is_empty() {
            self.content.push(String::new());
            return;
        }

        let last_idx = self.content.len() - 1;
        if !self.content[last_idx].is_empty() {
            self.content.push(String::new());
        } else if self.content.len() == 1 && self.content[0].is_empty() {
            // Already has exactly one empty line
        } else if last_idx > 0
            && self.content[last_idx - 1].is_empty()
            && self.content[last_idx].is_empty()
        {
            self.content.truncate(last_idx + 1);
        }
    }
}

// --- Selection ---

impl EditorState {
    /// Returns `(start_y, start_x, end_y, end_x)` with start always before end.
    pub const fn get_selection_bounds(&self) -> (usize, usize, usize, usize) {
        if !self.selection_active {
            return (self.cursor_y, self.cursor_x, self.cursor_y, self.cursor_x);
        }

        let (start_y, start_x, end_y, end_x) =
            if (self.selection_start_y < self.cursor_y)
                || (self.selection_start_y == self.cursor_y
                    && self.selection_start_x < self.cursor_x)
            {
                (
                    self.selection_start_y,
                    self.selection_start_x,
                    self.cursor_y,
                    self.cursor_x,
                )
            } else {
                (
                    self.cursor_y,
                    self.cursor_x,
                    self.selection_start_y,
                    self.selection_start_x,
                )
            };

        (start_y, start_x, end_y, end_x)
    }

    /// Return the text within the current selection, or the current line if no selection.
    pub fn get_selected_text(&self) -> String {
        if !self.selection_active {
            return self
                .content
                .get(self.cursor_y)
                .map(|line| format!("{line}\n"))
                .unwrap_or_default();
        }

        let (start_y, start_x, end_y, end_x) = self.get_selection_bounds();

        if start_y == end_y {
            return self
                .content
                .get(start_y)
                .map(|line| {
                    let end = end_x.min(line.len());
                    if start_x <= end {
                        line[start_x..end].to_string()
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default();
        }

        let mut result = String::new();

        // First line
        if let Some(line) = self.content.get(start_y) {
            let start_idx = start_x.min(line.len());
            if start_idx < line.len() {
                result.push_str(&line[start_idx..]);
            }
            result.push('\n');
        }

        // Middle lines
        result.extend(
            self.content
                .iter()
                .skip(start_y + 1)
                .take(end_y - start_y - 1)
                .flat_map(|line| [line.as_str(), "\n"].into_iter()),
        );

        // Last line
        if let Some(line) = self.content.get(end_y) {
            let end_idx = end_x.min(line.len());
            result.push_str(&line[..end_idx]);
        }

        result
    }

    /// Delete the text within the current selection and place cursor at selection start.
    pub fn delete_selected_text(&mut self) {
        if !self.selection_active {
            return;
        }

        let (start_y, start_x, end_y, end_x) = self.get_selection_bounds();

        if start_y == end_y {
            if let Some(line) = self.content.get_mut(start_y) {
                let end_idx = end_x.min(line.len());
                if start_x < end_idx {
                    let after = line[end_idx..].to_string();
                    line.truncate(start_x);
                    line.push_str(&after);
                }
            }
        } else {
            let mut new_content = Vec::new();

            new_content.extend(self.content[0..start_y].iter().cloned());

            let first_part = self
                .content
                .get(start_y)
                .map_or_else(String::new, |line| {
                    line[..start_x.min(line.len())].to_string()
                });

            let last_part = self.content.get(end_y).map_or_else(String::new, |line| {
                let end_idx = end_x.min(line.len());
                line[end_idx..].to_string()
            });

            new_content.push(first_part + &last_part);

            new_content.extend(self.content[end_y + 1..].iter().cloned());

            self.content = new_content;
        }

        self.cursor_y = start_y;
        self.cursor_x = start_x;
        self.selection_active = false;
    }
}

// --- Clipboard ---

impl EditorState {
    /// Write text to system clipboard (silently ignores errors).
    pub fn clipboard_write(text: &str) {
        let _ = Clipboard::new().and_then(|mut cb| cb.set_text(text.to_owned()));
    }

    /// Read text from system clipboard.
    pub fn clipboard_read() -> Option<String> {
        Clipboard::new().ok()?.get_text().ok()
    }

    /// Copy the current selection (or line) to the system clipboard.
    pub fn copy_selection(&self) {
        if !self.selection_active {
            if let Some(line) = self.content.get(self.cursor_y) {
                Self::clipboard_write(&format!("{line}\n"));
            }
            return;
        }

        let selected_text = self.get_selected_text();
        Self::clipboard_write(&selected_text);
    }

    /// Cut selection or current line to clipboard.
    pub fn cut_selection(&mut self) {
        if self.selection_active {
            let selected_text = self.get_selected_text();
            if !selected_text.is_empty() {
                Self::clipboard_write(&selected_text);
                self.delete_selected_text();
            }
        } else if !self.content.is_empty() && self.cursor_y < self.content.len() {
            let line = self.content.remove(self.cursor_y);
            Self::clipboard_write(&(line + "\n"));

            if self.content.is_empty() {
                self.content.push(String::new());
            }

            if self.cursor_y >= self.content.len() {
                self.cursor_y = self.content.len() - 1;
            }
            self.cursor_x = 0;
        }
        self.selection_active = false;
    }

    /// Paste from clipboard at current cursor position.
    pub fn paste_from_clipboard(&mut self) {
        if self.selection_active {
            self.delete_selected_text();
            self.selection_active = false;
        }

        if let Some(content) = Self::clipboard_read() {
            let normalized_content = content.replace("\r\n", "\n");
            let lines: Vec<&str> = normalized_content.split('\n').collect();
            let line_count = lines.len();

            if line_count == 1 || (line_count == 2 && lines[1].is_empty()) {
                if let Some(line) = self.content.get_mut(self.cursor_y) {
                    if self.cursor_x > line.len() {
                        line.push_str(&" ".repeat(self.cursor_x - line.len()));
                    }
                    line.insert_str(self.cursor_x, lines[0]);
                    self.cursor_x += lines[0].len();
                }
            } else {
                let current_line = self.content.get(self.cursor_y).map_or_else(
                    || (String::new(), String::new()),
                    |line| {
                        let x = self.cursor_x.min(line.len());
                        (line[..x].to_string(), line[x..].to_string())
                    },
                );

                if self.cursor_y < self.content.len() {
                    self.content[self.cursor_y] = current_line.0 + lines[0];
                }

                let mut insert_pos = self.cursor_y + 1;

                for &line in &lines[1..line_count - 1] {
                    self.insert_or_append_at(insert_pos, line.to_string());
                    insert_pos += 1;
                }

                if line_count > 1 {
                    let last_line = lines[line_count - 1];
                    let new_line = last_line.to_string() + &current_line.1;

                    self.insert_or_append_at(insert_pos, new_line);

                    self.cursor_y = insert_pos;
                    self.cursor_x = last_line.len();
                }
            }
        }
    }
}

// --- Stanza / paragraph analysis ---

impl EditorState {
    /// Returns the `(start_line, end_line)` bounds of the paragraph containing the cursor.
    pub fn get_current_paragraph_bounds(&self) -> Option<(usize, usize)> {
        let y = self.cursor_y;

        if y >= self.content.len() {
            return None;
        }

        let start_y = (0..=y)
            .rev()
            .find(|&i| i == 0 || self.content.get(i - 1).is_some_and(String::is_empty))
            .unwrap_or(y);

        if self.content.get(start_y).is_none_or(String::is_empty) {
            return None;
        }

        let end_y = (y..self.content.len())
            .find(|&i| self.content.get(i).is_none_or(String::is_empty))
            .map_or(self.content.len() - 1, |i| i.saturating_sub(1));

        if start_y <= end_y {
            Some((start_y, end_y))
        } else {
            None
        }
    }

    /// Check if cursor is currently in a stanza (non-empty text region).
    pub fn is_cursor_in_stanza(&self) -> bool {
        let cursor_y = self.cursor_y;

        let current_line_empty = self
            .content
            .get(cursor_y)
            .is_none_or(String::is_empty);

        if !current_line_empty {
            return true;
        }

        let has_text_above = self
            .content
            .iter()
            .take(cursor_y)
            .rev()
            .take_while(|line| line.is_empty())
            .count()
            < cursor_y;

        let has_text_below = self
            .content
            .iter()
            .skip(cursor_y + 1)
            .take_while(|line| line.is_empty())
            .count()
            < self.content.len() - cursor_y - 1;

        has_text_above && has_text_below
    }

    /// Find the start of the stanza containing line `y`.
    pub fn find_stanza_start(&self, y: usize) -> usize {
        self.content
            .iter()
            .take(y)
            .enumerate()
            .rev()
            .find_map(|(i, line)| {
                if line.is_empty() {
                    Some(i + 1)
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }
}

// --- Verse markers ---

impl EditorState {
    /// Insert a verse marker (e.g., "[Verse 1]") with appropriate blank line handling.
    pub fn insert_verse_marker(&mut self, marker_text: &str) {
        let marker_line = format!("[{marker_text}]");
        let cursor_y = self.cursor_y;
        let content_len = self.content.len();

        let is_touching_stanza = self.is_cursor_in_stanza()
            || (cursor_y > 0
                && cursor_y < content_len
                && self
                    .content
                    .get(cursor_y - 1)
                    .is_some_and(|line| !line.is_empty())
                && self
                    .content
                    .get(cursor_y)
                    .is_some_and(String::is_empty));

        if is_touching_stanza {
            let original_cursor_y = cursor_y;
            let original_cursor_x = self.cursor_x;

            let effective_y = if self.is_cursor_in_stanza() {
                cursor_y
            } else {
                cursor_y - 1
            };
            let stanza_start = self.find_stanza_start(effective_y);
            let mut insert_pos = stanza_start;
            let mut lines_inserted_above = 0;

            if stanza_start > 0
                && self
                    .content
                    .get(stanza_start - 1)
                    .is_some_and(|line| !line.is_empty())
            {
                self.content.insert(insert_pos, String::new());
                insert_pos += 1;
                lines_inserted_above += 1;
            }

            self.content.insert(insert_pos, marker_line);
            lines_inserted_above += 1;

            self.cursor_y = original_cursor_y + lines_inserted_above;
            self.cursor_x = original_cursor_x;
        } else {
            let mut marker_idx = cursor_y;

            if cursor_y < content_len && self.content[cursor_y].is_empty() {
                self.content[cursor_y] = marker_line;
            } else {
                self.content.insert(cursor_y, marker_line);
            }

            if marker_idx > 0
                && self
                    .content
                    .get(marker_idx - 1)
                    .is_some_and(|line| !line.is_empty())
            {
                self.content.insert(marker_idx, String::new());
                marker_idx += 1;
            }

            let after_idx = marker_idx + 1;
            if after_idx >= self.content.len() {
                self.content.push(String::new());
            } else if self
                .content
                .get(after_idx)
                .is_some_and(|line| !line.is_empty())
            {
                self.content.insert(after_idx, String::new());
            }

            let target_y = marker_idx + 2;
            while target_y >= self.content.len() {
                self.content.push(String::new());
            }
            self.cursor_y = target_y;
            self.cursor_x = 0;
        }

        // Final clamp and ensure trailing empty line
        self.cursor_y = self.cursor_y.min(self.content.len().saturating_sub(1));
        self.ensure_empty_line_at_end();
    }
}

// --- Viewport synchronization ---

impl EditorState {
    /// Synchronize editor state with the current viewport dimensions.
    ///
    /// Updates the wrap column when viewport width changes (if auto-wrap is
    /// enabled) and records the viewport height for scroll calculations.
    /// Call this once per frame when the editor layout is computed.
    pub fn sync_viewport(&mut self, viewport_width: usize, viewport_height: usize) {
        if self.last_viewport_width != Some(viewport_width) {
            self.last_viewport_width = Some(viewport_width);
            if self.wrap_auto {
                self.wrap_column = viewport_width.saturating_sub(2).max(MIN_WRAP_COLUMN);
            }
        }
        self.viewport_height = viewport_height;
    }

    /// Adjust scroll offset so the cursor's visual line stays visible.
    ///
    /// `cursor_visual_y` is the cursor's position in visual-line space
    /// (after soft-wrapping), not content-line space.
    pub const fn adjust_scroll(&mut self, cursor_visual_y: usize) {
        if cursor_visual_y < self.scroll_offset {
            self.scroll_offset = cursor_visual_y;
        } else if cursor_visual_y >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = cursor_visual_y.saturating_sub(self.viewport_height - 1);
        }
    }
}

// --- Verse marker parsing (needs external verse_groups) ---

impl EditorState {
    /// Parse a command string against verse groups to produce a marker name.
    ///
    /// Accepts `verse_groups` as a parameter because the groups are owned by `App`.
    pub fn parse_verse_marker(command: &str, verse_groups: &[VerseGroup]) -> Option<String> {
        for group in verse_groups {
            if command.starts_with(&group.command) {
                let remainder = &command[group.command.len()..];

                if remainder.is_empty() {
                    return Some(group.name.clone());
                }

                if let Ok(num) = remainder.parse::<u32>() {
                    return Some(format!("{} {num}", group.name));
                }
            }
        }
        None
    }
}
