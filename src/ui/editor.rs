use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, symbols,
};

use crate::app::{App, VerseGroup};

pub fn draw_editor(f: &mut Frame, app: &mut App, area: Rect) {
    // Create a layout for the editor area - with wrap guide line
    let editor_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
        ])
        .split(area);
    
    // Draw a block around the whole editor
    let editor_block = Block::default()
        .title(Span::styled("Editor", Style::default().fg(Color::Yellow)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    
    f.render_widget(editor_block.clone(), editor_layout[0]);
    
    // Get the inner area for the editor content
    let inner_area = editor_block.inner(editor_layout[0]);
    
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
    let paragraph = Paragraph::new(styled_content)
        .wrap(Wrap { trim: false })
        .scroll((0, 0));
    
    f.render_widget(paragraph, inner_area);
    
    // Draw the wrap guide if we're in the editor
    draw_wrap_guide(f, app, inner_area);
    
    // Draw verse reference box in the bottom right
    draw_verse_reference(f, app, inner_area);
    
    // Show cursor only when we're in the main editor (not in command mode)
    if !app.editor.is_command_mode {
        let cursor_y = app.editor.cursor_y.saturating_sub(app.editor.scroll_offset) as u16;
        if cursor_y < inner_area.height {
            f.set_cursor(
                inner_area.left() + app.editor.cursor_x as u16,
                inner_area.top() + cursor_y
            );
        }
    }
    
    // Draw editor command line if in command mode
    if app.editor.is_command_mode {
        let command_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner_area)[1];
        
        let command = Paragraph::new(format!(":{}",app.editor.command_buffer))
            .style(Style::default().fg(Color::Yellow));
        
        f.render_widget(command, command_area);
        
        // Set the cursor position in the command line
        f.set_cursor(
            command_area.left() + app.editor.command_buffer.len() as u16 + 1,
            command_area.top()
        );
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
    
    // Only draw if wrap column is within visible area
    if wrap_col <= area.width as usize {
        // Draw a vertical line at the wrap column
        for y in 0..area.height {
            let pos = area.left() + wrap_col as u16;
            let cell_pos = Rect::new(pos, area.top() + y, 1, 1);
            
            f.buffer_mut().set_string(
                cell_pos.x,
                cell_pos.y,
                symbols::line::VERTICAL,
                Style::default().fg(Color::DarkGray)
            );
        }
    }
}

fn draw_verse_reference(f: &mut Frame, app: &App, area: Rect) {
    // Create a box in the bottom right for verse reference info
    let verse_ref_width = 20;
    let verse_ref_height = 8;
    
    // Only show verse reference if there's enough space
    if area.width > verse_ref_width + 2 && area.height > verse_ref_height + 2 {
        let verse_ref_area = Rect {
            x: area.right() - verse_ref_width - 2,
            y: area.bottom() - verse_ref_height - 1,
            width: verse_ref_width,
            height: verse_ref_height,
        };
        
        let verse_ref_block = Block::default()
            .title("Verse Markers")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        
        f.render_widget(verse_ref_block.clone(), verse_ref_area);
        
        // Show verse marker commands in the box
        let inner_area = verse_ref_block.inner(verse_ref_area);
        let mut verse_items = Vec::new();
        
        for (_i, verse_group) in app.verse_groups.iter().enumerate().take(verse_ref_height as usize - 1) {
            // Format for display: cmd -> name
            let display_text = format!(":{} → {}", verse_group.command, verse_group.name);
            
            // Add to list
            verse_items.push(Line::from(Span::styled(
                display_text,
                Style::default().fg(verse_group.color)
            )));
        }
        
        let verse_paragraph = Paragraph::new(verse_items)
            .style(Style::default())
            .alignment(ratatui::layout::Alignment::Left);
        
        f.render_widget(verse_paragraph, inner_area);
    }
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
