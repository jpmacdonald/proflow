use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, symbols,
    layout::Alignment,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, VerseGroup, SlideType};
use crate::bible::BibleVersion;

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
    
    // Track viewport width for auto wrap (and trigger wrap on change)
    let new_width = editor_area.width as usize;
    if app.editor.last_viewport_width != Some(new_width) {
        app.editor.last_viewport_width = Some(new_width);
        app.apply_wrap_to_editor();
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
    
    // Update the viewport height so scrolling works correctly
    app.editor.viewport_height = inner_area.height as usize;
    
    // Calculate the visible portion of the content
    let start_line = app.editor.scroll_offset;
    let end_line = (app.editor.scroll_offset + inner_area.height as usize).min(app.editor.content.len());
    
    let visible_content: Vec<&String> = app.editor.content[start_line..end_line].iter().collect();
    
    // Prepare content with styled lines
    let mut styled_content = Vec::new();
    
    // Get the paragraph bounds for potential highlighting
    let paragraph_bounds = app.get_current_paragraph_bounds();

    // Convert selection coordinates to absolute positions for highlighting
    let selection_bounds = if app.editor.selection_active {
        let (start_y, start_x, end_y, end_x) = get_selection_bounds(app);
        Some((start_y, start_x, end_y, end_x))
    } else {
        None
    };
    
    // Create styled spans for each line
    for (i, line) in visible_content.iter().enumerate() {
        let abs_line_idx = start_line + i;
        
        let is_in_paragraph = paragraph_bounds
            .map_or(false, |(start, end)| abs_line_idx >= start && abs_line_idx <= end);
        
        // Set background based on paragraph, use a slightly brighter grey
        let base_fg_color = Color::White; 
        let base_bg_color = if is_in_paragraph {
            Color::Rgb(60, 60, 70) // Slightly brighter grey highlight
        } else {
            Color::Reset 
        };

        // Base style only carries foreground now
        let base_style = Style::default().fg(base_fg_color);
        
        styled_content.push(Line::from(
            // Pass background color explicitly AND verse groups
            style_editor_line(abs_line_idx, line, selection_bounds, base_style, base_bg_color, &app.verse_groups)
        ));
    }
    
    // Render the editor content
    // NO auto-wrap - user controls wrapping with :wrap command
    // This ensures cursor position matches display position
    let paragraph = Paragraph::new(styled_content)
        .wrap(Wrap { trim: false }) // soft-wrap for display; hard-wrap still controlled by :wrap
        .scroll((0, 0));
    
    f.render_widget(paragraph, inner_area);
    
    // Draw the wrap guide if we're in the editor
    draw_wrap_guide(f, app, inner_area);
    
    // Show cursor only when we're in the main editor (not in command mode) and not side pane focused
    if !app.editor.is_command_mode && !app.editor_side_pane_focused {
        let cursor_y = app.editor.cursor_y.saturating_sub(app.editor.scroll_offset) as u16;
        if cursor_y < inner_area.height {
            // Calculate display width up to cursor position (unicode-aware)
            let current_line = app.editor.content.get(app.editor.cursor_y).map(|s| s.as_str()).unwrap_or("");
            let prefix: String = current_line.chars().take(app.editor.cursor_x).collect();
            let display_x = prefix.width() as u16;
            
            // Bound cursor to visible area (prevent going into side pane)
            let bounded_x = display_x.min(inner_area.width.saturating_sub(1));
            
            f.set_cursor(
                inner_area.left() + bounded_x,
                inner_area.top() + cursor_y
            );
        }
    }
    
}

fn style_editor_line(
    y: usize,
    line_content: &str,
    selection_bounds: Option<(usize, usize, usize, usize)>,
    base_style: Style, // Only carries FG
    base_bg_color: Color, // Explicit background color
    verse_groups: &[VerseGroup] // Add verse_groups parameter
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let selection_highlight_style = Style::default().bg(Color::Rgb(80, 80, 120)).fg(Color::White);

    // Verse markers
    if line_content.starts_with('[') && line_content.contains(']') {
        let marker_end = line_content.find(']').unwrap_or(line_content.len());
        let marker_text = &line_content[1..marker_end];
        let rest_of_line = &line_content[marker_end+1..];
        
        // Marker has no background - Pass actual verse_groups now
        spans.push(style_verse_marker(marker_text, line_content, verse_groups)); 
        
        if !rest_of_line.is_empty() {
            if let Some((start_y, start_x, end_y, end_x)) = selection_bounds {
                if y >= start_y && y <= end_y {
                    let marker_len = marker_end + 1;
                    let sel_start_rel = start_x.saturating_sub(marker_len);
                    let sel_end_rel = end_x.saturating_sub(marker_len);
                    if y == start_y && y == end_y { 
                        let start = sel_start_rel.max(0).min(rest_of_line.len()); let end = sel_end_rel.max(0).min(rest_of_line.len());
                        if start < end { add_selection_spans_owned(&mut spans, rest_of_line, start, end, base_style, base_bg_color, selection_highlight_style); return spans; }
                    } else if y == start_y { 
                         let start = sel_start_rel.max(0).min(rest_of_line.len());
                        if start < rest_of_line.len() { add_selection_spans_owned(&mut spans, rest_of_line, start, rest_of_line.len(), base_style, base_bg_color, selection_highlight_style); return spans; }
                    } else if y == end_y { 
                        let end = sel_end_rel.max(0).min(rest_of_line.len());
                        if end > 0 { add_selection_spans_owned(&mut spans, rest_of_line, 0, end, base_style, base_bg_color, selection_highlight_style); return spans; }
                    } else { 
                        spans.push(Span::styled(rest_of_line.to_string(), selection_highlight_style)); return spans;
                    }
                }
            }
            spans.push(Span::styled(rest_of_line.to_string(), base_style.bg(base_bg_color)));
        }
        return spans;
    }
    
    // Regular line
    if let Some((start_y, start_x, end_y, end_x)) = selection_bounds {
        if y >= start_y && y <= end_y {
            if y == start_y && y == end_y { 
                 let start = start_x.min(line_content.len()); let end = end_x.min(line_content.len());
                if start < end { add_selection_spans_owned(&mut spans, line_content, start, end, base_style, base_bg_color, selection_highlight_style); return spans; }
            } else if y == start_y { 
                 let start = start_x.min(line_content.len());
                if start < line_content.len() { add_selection_spans_owned(&mut spans, line_content, start, line_content.len(), base_style, base_bg_color, selection_highlight_style); return spans; }
            } else if y == end_y { 
                 let end = end_x.min(line_content.len());
                if end > 0 { add_selection_spans_owned(&mut spans, line_content, 0, end, base_style, base_bg_color, selection_highlight_style); return spans; }
            } else { 
                spans.push(Span::styled(line_content.to_string(), selection_highlight_style)); return spans;
            }
        }
    }
    
    spans.push(Span::styled(line_content.to_string(), base_style.bg(base_bg_color)));
    spans
}

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
        SlideType::Lyrics => draw_markers_pane(f, app, area),
        _ => draw_markers_pane(f, app, area), // Default to markers for other types
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
                Span::styled(format!("{:<8}", name), style), // pad names for justification
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
        .map(|group| group.color)
        .unwrap_or(Color::Yellow); // Default to Yellow
    
    // Make verse markers bold and use the determined color (likely Yellow)
    Span::styled(
        format!("[{}]", marker_text),
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

fn get_selection_bounds(app: &App) -> (usize, usize, usize, usize) {
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
