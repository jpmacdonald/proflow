use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, symbols,
    layout::Alignment,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, VerseGroup, SlideType};
use crate::bible::BibleVersion;
use crate::constants::editor::MIN_WRAP_COLUMN;

/// A visual line with its source content line index and character offset
#[derive(Debug, Clone)]
struct VisualLine {
    /// Index into content Vec
    content_line: usize,
    /// Character offset within the content line where this visual line starts
    char_start: usize,
    /// The text to display (this visual line's portion)
    text: String,
}

/// Compute visual lines from content with soft-wrapping
fn compute_visual_lines(content: &[String], wrap_column: usize) -> Vec<VisualLine> {
    let mut visual_lines = Vec::new();
    let wrap_width = wrap_column.max(MIN_WRAP_COLUMN);
    
    for (content_idx, line) in content.iter().enumerate() {
        if line.is_empty() {
            // Empty lines stay as single visual line
            visual_lines.push(VisualLine {
                content_line: content_idx,
                char_start: 0,
                text: String::new(),
            });
            continue;
        }
        
        // Wrap long lines by character width
        let mut char_start = 0;
        let chars: Vec<char> = line.chars().collect();
        
        while char_start < chars.len() {
            let mut visual_width = 0;
            let mut char_end = char_start;
            
            // Accumulate characters until we hit wrap width
            while char_end < chars.len() {
                let ch = chars[char_end];
                let ch_width = UnicodeWidthStr::width(ch.to_string().as_str());
                if visual_width + ch_width > wrap_width && char_end > char_start {
                    break;
                }
                visual_width += ch_width;
                char_end += 1;
            }
            
            let segment: String = chars[char_start..char_end].iter().collect();
            visual_lines.push(VisualLine {
                content_line: content_idx,
                char_start,
                text: segment,
            });
            
            char_start = char_end;
        }
    }
    
    // Ensure at least one visual line
    if visual_lines.is_empty() {
        visual_lines.push(VisualLine {
            content_line: 0,
            char_start: 0,
            text: String::new(),
        });
    }
    
    visual_lines
}

/// Map cursor position (`content_line`, `char_offset`) to visual line index and x position.
fn cursor_to_visual(
    content_cursor_y: usize,
    content_cursor_x: usize,
    visual_lines: &[VisualLine],
) -> (usize, usize) {
    for (visual_idx, vl) in visual_lines.iter().enumerate() {
        if vl.content_line == content_cursor_y {
            let char_end = vl.char_start + vl.text.chars().count();
            if content_cursor_x >= vl.char_start && content_cursor_x <= char_end {
                // Cursor is on this visual line
                let visual_x = content_cursor_x - vl.char_start;
                return (visual_idx, visual_x);
            }
        }
    }
    // Fallback: last visual line for this content line
    for (visual_idx, vl) in visual_lines.iter().enumerate().rev() {
        if vl.content_line == content_cursor_y {
            let visual_x = vl.text.chars().count();
            return (visual_idx, visual_x);
        }
    }
    (0, 0)
}

/// Render the text editor view with side pane, cursor, and selection highlighting.
#[allow(clippy::cast_possible_truncation, clippy::similar_names)]
pub fn draw_editor(f: &mut Frame, app: &mut App, area: Rect) {
    // Split into main editor and side pane
    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(40),       // Main editor
            Constraint::Length(22),    // Side pane
        ])
        .split(area);
    
    let editor_area = main_layout[0];
    let side_pane_area = main_layout[1];
    
    // Track viewport width for auto wrap column calculation
    let new_width = editor_area.width as usize;
    if app.editor.last_viewport_width != Some(new_width) {
        app.editor.last_viewport_width = Some(new_width);
        app.update_wrap_column_from_viewport();
    }

    // Draw the side pane based on slide type
    draw_side_pane(f, app, side_pane_area);
    
    // Build editor title - include scripture reference if available
    let title = match (&app.current_slide_type, &app.current_scripture_header) {
        (SlideType::Scripture, Some(header)) => format!("Editor [{}] │ {}", app.current_slide_type.name(), header.display()),
        _ => format!("Editor [{}]", app.current_slide_type.name()),
    };
    
    let border_color = if app.editor_side_pane_focused { Color::DarkGray } else { Color::Yellow };
    let editor_block = Block::default()
        .title(Span::styled(title, Style::default().fg(Color::Yellow)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    
    f.render_widget(editor_block.clone(), editor_area);
    
    // Get the inner area for the editor content
    let inner_area = editor_block.inner(editor_area);
    
    // Compute visual lines with soft-wrapping
    let visual_lines = compute_visual_lines(&app.editor.content, app.editor.wrap_column);
    let total_visual_lines = visual_lines.len();
    
    // Update the viewport height
    app.editor.viewport_height = inner_area.height as usize;
    
    // Map cursor to visual line for scroll adjustment
    let (cursor_visual_y, cursor_visual_x) = cursor_to_visual(
        app.editor.cursor_y,
        app.editor.cursor_x,
        &visual_lines,
    );
    
    // Adjust scroll offset to keep cursor visible (in visual line space)
    if cursor_visual_y < app.editor.scroll_offset {
        app.editor.scroll_offset = cursor_visual_y;
    } else if cursor_visual_y >= app.editor.scroll_offset + app.editor.viewport_height {
        app.editor.scroll_offset = cursor_visual_y.saturating_sub(app.editor.viewport_height - 1);
    }
    
    // Calculate visible visual lines
    let start_visual = app.editor.scroll_offset;
    let end_visual = (start_visual + inner_area.height as usize).min(total_visual_lines);
    
    // Get the paragraph bounds for highlighting (in content line space)
    let paragraph_bounds = app.get_current_paragraph_bounds();

    // Convert selection coordinates to absolute positions for highlighting
    let selection_bounds = if app.editor.selection_active {
        let (start_y, start_x, end_y, end_x) = get_selection_bounds(app);
        Some((start_y, start_x, end_y, end_x))
    } else {
        None
    };
    
    // Prepare content with styled lines
    let mut styled_content = Vec::new();
    
    for vl in &visual_lines[start_visual..end_visual] {
        let content_line_idx = vl.content_line;

        let is_in_paragraph = paragraph_bounds
            .is_some_and(|(start, end)| content_line_idx >= start && content_line_idx <= end);

        let base_fg = Color::White;
        let base_bg = if is_in_paragraph {
            Color::Rgb(60, 60, 70)
        } else {
            Color::Reset 
        };

        let base_style = Style::default().fg(base_fg);

        // Style this visual line segment
        styled_content.push(Line::from(
            style_visual_line(
                vl,
                selection_bounds,
                base_style,
                base_bg,
                &app.verse_groups
            )
        ));
    }
    
    // Render the editor content (no additional wrapping needed - we did it ourselves)
    let paragraph = Paragraph::new(styled_content);
    f.render_widget(paragraph, inner_area);
    
    // Draw the wrap guide
    draw_wrap_guide(f, app, inner_area);
    
    // Show cursor only when we're in the main editor (not in command mode) and not side pane focused
    if !app.editor.is_command_mode && !app.editor_side_pane_focused {
        let cursor_display_y = cursor_visual_y.saturating_sub(app.editor.scroll_offset) as u16;
        
        if cursor_display_y < inner_area.height {
            // Calculate display width for cursor x position
            let prefix_chars = cursor_visual_x;
            let display_x = visual_lines.get(cursor_visual_y).map_or(0, |vl| {
                let prefix: String = vl.text.chars().take(prefix_chars).collect();
                prefix.width() as u16
            });
            
            // Bound cursor to visible area
            let bounded_x = display_x.min(inner_area.width.saturating_sub(1));
            
            f.set_cursor(
                inner_area.left() + bounded_x,
                inner_area.top() + cursor_display_y
            );
        }
    }
}

/// Style a visual line segment for display
fn style_visual_line(
    vl: &VisualLine,
    selection_bounds: Option<(usize, usize, usize, usize)>, // content line coordinates
    base_style: Style,
    base_bg_color: Color,
    verse_groups: &[VerseGroup],
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let selection_highlight_style = Style::default().bg(Color::Rgb(80, 80, 120)).fg(Color::White);
    let line_content = &vl.text;
    let content_y = vl.content_line;
    let char_offset = vl.char_start;

    // Handle verse markers only if this is the start of the content line
    if char_offset == 0 && line_content.starts_with('[') && line_content.contains(']') {
        let marker_end = line_content.find(']').unwrap_or(line_content.len());
        let marker_text = &line_content[1..marker_end];
        let rest_of_line = &line_content[marker_end+1..];
        
        spans.push(style_verse_marker(marker_text, line_content, verse_groups)); 
        
        if !rest_of_line.is_empty() {
            // Handle selection for the rest of line after marker
            if let Some((start_y, start_x, end_y, end_x)) = selection_bounds {
                if content_y >= start_y && content_y <= end_y {
                    let marker_len = marker_end + 1;
                    // Adjust selection coords for visual segment
                    let seg_start = if content_y == start_y { start_x.saturating_sub(marker_len) } else { 0 };
                    let seg_end = if content_y == end_y { end_x.saturating_sub(marker_len) } else { rest_of_line.len() };
                    let seg_start = seg_start.min(rest_of_line.len());
                    let seg_end = seg_end.min(rest_of_line.len());
                    if seg_start < seg_end {
                        add_selection_spans_owned(&mut spans, rest_of_line, seg_start, seg_end, base_style, base_bg_color, selection_highlight_style);
                        return spans;
                    }
                }
            }
            spans.push(Span::styled(rest_of_line.to_string(), base_style.bg(base_bg_color)));
        }
        return spans;
    }
    
    // Regular visual line segment - handle selection
    if let Some((start_y, start_x, end_y, end_x)) = selection_bounds {
        if content_y >= start_y && content_y <= end_y {
            // Convert content-level selection to this visual segment
            let _seg_char_end = char_offset + line_content.chars().count();
            
            // Selection start/end in this content line
            let sel_start_in_line = if content_y == start_y { start_x } else { 0 };
            let sel_end_in_line = if content_y == end_y { end_x } else { usize::MAX };
            
            // Clip to this visual segment
            let seg_sel_start = sel_start_in_line.saturating_sub(char_offset).min(line_content.len());
            let seg_sel_end = sel_end_in_line.saturating_sub(char_offset).min(line_content.len());
            
            if seg_sel_start < seg_sel_end && seg_sel_end > 0 {
                add_selection_spans_owned(&mut spans, line_content, seg_sel_start, seg_sel_end, base_style, base_bg_color, selection_highlight_style);
                return spans;
            }
        }
    }
    
    spans.push(Span::styled(line_content.clone(), base_style.bg(base_bg_color)));
    spans
}

#[allow(clippy::cast_possible_truncation)]
fn draw_wrap_guide(f: &mut Frame, app: &App, area: Rect) {
    let wrap_col = app.editor.wrap_column;
    
    let (draw_col, ch) = if wrap_col < area.width as usize {
        (wrap_col as u16, symbols::line::VERTICAL)
    } else {
        // Show at right edge with indicator when beyond visible width
        (area.width.saturating_sub(1), "»")
    };

        for y in 0..area.height {
        let pos = area.left() + draw_col;
            f.buffer_mut().set_string(
            pos,
            area.top() + y,
            ch,
            Style::default().fg(Color::Cyan)
        );
    }
}

/// Draw the side pane based on current slide type
fn draw_side_pane(f: &mut Frame, app: &App, area: Rect) {
    match app.current_slide_type {
        SlideType::Scripture => draw_version_pane(f, app, area),
        _ => draw_markers_pane(f, app, area),
    }
}

/// Draw Bible version selector for Scripture mode
fn draw_version_pane(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.editor_side_pane_focused;
    let border_color = if is_focused { Color::Yellow } else { Color::DarkGray };
    let title_color = if is_focused { Color::Yellow } else { Color::Gray };
    
    let block = Block::default()
        .title(Span::styled("Versions", Style::default().fg(title_color)))
            .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    
    f.render_widget(block.clone(), area);
    let inner = block.inner(area);
    
    let versions = BibleVersion::all();
    let lines: Vec<Line> = versions.iter()
        .enumerate()
        .map(|(i, v)| {
            let is_selected = i == app.version_picker_selection;
            let arrow = if is_selected { "▶" } else { " " };
            let key = format!("{:>2}", i + 1); // right-align numbers
            let style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            // Pad right to a fixed width for alignment, then center the paragraph
            let name = v.name();
            Line::from(vec![
                Span::styled(arrow, style),
                Span::raw(" "),
                Span::styled(key, Style::default().fg(Color::Yellow)),
                Span::raw(" "),
                Span::styled(format!("{name:<8}"), style), // pad names for justification
            ])
        })
        .collect();
    
    // Add hint at bottom
    let mut all_lines = lines;
    all_lines.push(Line::from(""));
    all_lines.push(Line::from(Span::styled("1-4: switch", Style::default().fg(Color::DarkGray))));
    
    let paragraph = Paragraph::new(all_lines).alignment(Alignment::Center);
    f.render_widget(paragraph, inner);
}

/// Draw verse marker shortcuts for Lyrics mode
fn draw_markers_pane(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.editor_side_pane_focused;
    let border_color = if is_focused { Color::Yellow } else { Color::DarkGray };
    let title_color = if is_focused { Color::Yellow } else { Color::Gray };
    
    let block = Block::default()
        .title(Span::styled("Markers", Style::default().fg(title_color)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    
    f.render_widget(block.clone(), area);
    let inner = block.inner(area);
    
    let mut lines: Vec<Line> = app.verse_groups.iter()
        .enumerate()
        .take(inner.height as usize - 2)
        .map(|(i, group)| {
            let is_selected = i == app.editor_side_pane_idx;
            let prefix = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default().fg(group.color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(group.color)
            };
            Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!(":{}", group.command), Style::default().fg(Color::Yellow)),
                Span::raw(" → "),
                Span::styled(&group.name, style),
            ])
        })
        .collect();
    
    // Add hint
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Enter: insert", Style::default().fg(Color::DarkGray))));
    
    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn style_verse_marker(marker_text: &str, _line_content: &str, verse_groups: &[VerseGroup]) -> Span<'static> {
    // Find a matching verse group to get the color, but default to Yellow
    let color = verse_groups.iter()
        .find(|group| marker_text.starts_with(&group.name))
        .map_or(Color::Yellow, |group| group.color);
    
    // Make verse markers bold and use the determined color (likely Yellow)
    Span::styled(
        format!("[{marker_text}]"),
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    )
}

fn add_selection_spans_owned(
    spans: &mut Vec<Span<'static>>, 
    line_content: &str, 
    start_idx: usize, 
    end_idx: usize, 
    base_style: Style, 
    base_bg_color: Color, 
    selection_style: Style
) {
    let line_len = line_content.len();
    let start = start_idx.min(line_len);
    let end = end_idx.min(line_len);
    
    // Style for non-selected parts: Base FG + Explicit BG
    let non_selected_style = base_style.bg(base_bg_color);
    
    if start > 0 {
        spans.push(Span::styled(line_content[..start].to_string(), non_selected_style));
    }
    if end > start {
        spans.push(Span::styled(line_content[start..end].to_string(), selection_style));
    }
    if end < line_len {
        spans.push(Span::styled(line_content[end..].to_string(), non_selected_style));
    }
    if line_content.is_empty() {
        spans.push(Span::styled("", non_selected_style)); 
    }
}

const fn get_selection_bounds(app: &App) -> (usize, usize, usize, usize) {
    if !app.editor.selection_active {
        // If no selection, return cursor position for both start and end
        return (
            app.editor.cursor_y, 
            app.editor.cursor_x, 
            app.editor.cursor_y, 
            app.editor.cursor_x
        );
    }
    
    // Determine start and end points based on selection direction
    let (start_y, start_x, end_y, end_x) = if (app.editor.selection_start_y < app.editor.cursor_y) || 
        (app.editor.selection_start_y == app.editor.cursor_y && app.editor.selection_start_x < app.editor.cursor_x) {
        // Normal selection (top to bottom)
        (
            app.editor.selection_start_y, 
            app.editor.selection_start_x, 
            app.editor.cursor_y, 
            app.editor.cursor_x
        )
    } else {
        // Reverse selection (bottom to top)
        (
            app.editor.cursor_y, 
            app.editor.cursor_x, 
            app.editor.selection_start_y, 
            app.editor.selection_start_x
        )
    };
    
    (start_y, start_x, end_y, end_x)
}
