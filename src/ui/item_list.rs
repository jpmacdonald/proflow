use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::app::{App, SlideType};
use crate::ui::create_titled_block;

pub fn draw_item_list(f: &mut Frame, app: &mut App, area: Rect) {
    // Changed to vertical layout - files appear below items now
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(area);

    let selected_item_index = app.item_list_state.selected();
    let selected_file_index = app.file_list_state.selected();
    let items_is_focused = selected_file_index.is_none();
    let files_is_focused = selected_file_index.is_some();

    // Define selection style colors
    let selection_bg = Color::Rgb(80, 80, 120); // Background for selected item (focused or not)
    let default_bg = Color::Reset;
    let focused_fg = Color::Yellow; // Foreground for selected item when focused
    let default_fg = Color::White;

    // Define slide type indicators and their colors
    let slide_type_indicators = [
        (SlideType::Scripture, "[ Scripture ]", Color::Cyan),
        (SlideType::Lyrics, "[ Lyrics ]", Color::Green),
        (SlideType::Title, "[ Title ]", Color::Yellow),
        (SlideType::Graphic, "[ Graphic ]", Color::Magenta),
        (SlideType::Text, "[ Text ]", Color::Blue),
    ];
    
    let max_indicator_width = slide_type_indicators.iter().map(|(_, text, _)| text.chars().count()).max().unwrap_or(15);
    
    // Note: file list alignment offset available if needed
    // Width before Item Title = Prefix(2) + Status(2) + MaxCategoryWidth + Space(1)
    let _file_list_offset = 2 + 2 + max_indicator_width + 1;

    // --- Top pane: Plan Items --- 
    let item_list: Vec<ListItem> = app.items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = Some(i) == selected_item_index;
            let is_focused = items_is_focused;
            let current_bg = if is_selected { selection_bg } else { default_bg };
            
            // Get detected/overridden slide type for this item
            let slide_type = app.get_slide_type_for_item(item);
            let (type_indicator, type_color) = slide_type_indicators.iter()
                .find(|(st, _, _)| *st == slide_type)
                .map(|(_, text, color)| (*text, *color))
                .unwrap_or(("[ Text ]", Color::Blue));
            let padding = " ".repeat(max_indicator_width.saturating_sub(type_indicator.chars().count()) + 1);
            
            let is_completed = *app.item_completion.get(&item.id).unwrap_or(&false);
            let is_ignored = *app.item_ignored.get(&item.id).unwrap_or(&false);
            
            // Check if item has custom editor content (mutually exclusive with file match)
            let has_editor_content = app.item_editor_state.get(&item.id)
                .and_then(|opt| opt.as_ref())
                .map(|state| state.content.iter().any(|line| !line.trim().is_empty()))
                .unwrap_or(false);
            
            let matched_file = app.item_matched_file.get(&item.id)
                .and_then(|opt_file| opt_file.as_ref());
            
            // Determine status display: Created vs Matched vs neither
            let status_display = if has_editor_content {
                " -> [Created]".to_string()
            } else if let Some(file_path) = matched_file {
                let path = std::path::Path::new(file_path);
                let filename = path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(file_path);
                format!(" -> {}", filename)
            } else {
                String::new()
            };

            // Determine base foreground and modifier
            let base_fg = if is_focused && is_selected { focused_fg } else { default_fg };
            let modifier = if is_focused && is_selected { Modifier::BOLD } else { Modifier::empty() };

            // Styles for individual parts
            let mut title_style = Style::default().fg(base_fg).add_modifier(modifier);
            let mut category_style = Style::default().fg(type_color).add_modifier(modifier);
            let mut status_style = Style::default();
            let mut padding_style = Style::default();
            
            // Status display color: Cyan for matched file, Magenta for created
            let mut status_display_style = if has_editor_content {
                Style::default().fg(Color::Magenta).add_modifier(modifier)
            } else {
                Style::default().fg(Color::Cyan).add_modifier(modifier)
            };

            if is_ignored {
                title_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
                category_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
                status_style = Style::default().fg(Color::DarkGray);
                status_display_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
                padding_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
            }
            
            // --- Create Spans --- 
            let mut spans = Vec::new();
            
            // Prefix
            let prefix = if is_focused && is_selected { "> " } else { "  " };
            spans.push(Span::raw(prefix)); 

            // Status Icon: ✓ = matched, ✎ = created, ✗ = ignored
            if is_ignored {
                spans.push(Span::styled("✗ ", Style::default().fg(Color::Red)));
            } else if has_editor_content {
                spans.push(Span::styled("✎ ", Style::default().fg(Color::Magenta)));
            } else if is_completed {
                spans.push(Span::styled("✓ ", Style::default().fg(Color::Green)));
            } else {
                spans.push(Span::styled("  ", status_style));
            }
            
            // Category Indicator
            spans.push(Span::styled(type_indicator, category_style));
            
            // Padding
            spans.push(Span::styled(padding, padding_style));
            
            // Title
            spans.push(Span::styled(item.title.clone(), title_style));
            
            // Status Display (Created or Matched file)
            spans.push(Span::styled(status_display, status_display_style));

            ListItem::new(Line::from(spans)).style(Style::default().bg(current_bg))
        })
        .collect();

    let items_list_widget = List::new(item_list)
        .block(create_titled_block("Items", items_is_focused));
    f.render_stateful_widget(items_list_widget, chunks[0], &mut app.item_list_state);

    // --- Bottom pane: Matching Files ---
    // Gap should be 2 spaces to account for the Status Icon column width in the items list
    let file_list_gap = "  "; // Two spaces gap

    let files: Vec<ListItem> = app
        .matching_files
        .iter()
        .enumerate()
        .map(|(i, file_entry)| {
            let is_selected = Some(i) == selected_file_index;
            let is_focused = files_is_focused;
            let current_bg = if is_selected { selection_bg } else { default_bg }; // Determine item background

            // Determine base foreground and modifier
            let base_fg = if is_focused && is_selected { focused_fg } else { default_fg };
            let modifier = if is_focused && is_selected { Modifier::BOLD } else { Modifier::empty() };
            let text_style = Style::default().fg(base_fg).add_modifier(modifier); // NO background
            
            // Prefix (No style needed, inherits from item)
            let prefix = if is_focused && is_selected { "> " } else { "  " };

            // Create spans: Prefix + Gap + Filename
            let spans = vec![
                Span::raw(prefix),
                Span::raw(file_list_gap), // Use single space gap
                Span::styled(&file_entry.display_name, text_style), // Filename text with its specific style
            ];
            
            // Apply background to the *entire* ListItem
            ListItem::new(Line::from(spans)).style(Style::default().bg(current_bg))
        })
        .collect();

    let files_list = List::new(files)
        .block(create_titled_block("Matching Files", files_is_focused));
    f.render_stateful_widget(files_list, chunks[1], &mut app.file_list_state);
}
