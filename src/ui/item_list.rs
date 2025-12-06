use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::app::App;
use crate::planning_center::types::Category;
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

    // Define category indicators and their colors
    let category_indicators = [
        (Category::Song, "[ Song ]", Color::Green),
        (Category::Text, "[ Text ]", Color::Blue),
        (Category::Title, "[ Title ]", Color::Yellow),
        (Category::Graphic, "[ Graphic ]", Color::Magenta),
        (Category::Other, "[ Other ]", Color::DarkGray),
    ];
    
    let max_indicator_width = category_indicators.iter().map(|(_, text, _)| text.len()).max().unwrap_or(10);
    
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
            let current_bg = if is_selected { selection_bg } else { default_bg }; // Determine item background
            
            let (type_indicator, type_color) = category_indicators.iter().find(|(cat, _, _)| *cat == item.category).map(|(_, text, color)| (*text, *color)).unwrap_or(("[ Other ]", Color::DarkGray));
            let padding = " ".repeat(max_indicator_width - type_indicator.len() + 1);
            let is_completed = *app.item_completion.get(&item.id).unwrap_or(&false);
            let is_ignored = *app.item_ignored.get(&item.id).unwrap_or(&false);
            let matched_file_display = app.item_matched_file.get(&item.id)
                .and_then(|opt_file| opt_file.as_ref())
                .map(|s| {
                    // Extract just the filename without extension
                    let path = std::path::Path::new(s);
                    let filename = path.file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or(s);
                    format!(" -> {}", filename)
                })
                .unwrap_or_default();

            // Determine base foreground and modifier for text parts
            let base_fg = if is_focused && is_selected { focused_fg } else { default_fg };
            let modifier = if is_focused && is_selected { Modifier::BOLD } else { Modifier::empty() };

            // Styles for individual parts (NO background here)
            let mut title_style = Style::default().fg(base_fg).add_modifier(modifier);
            let mut category_style = Style::default().fg(type_color).add_modifier(modifier);
            let mut status_style = Style::default(); // For dimming/crossing out space
            let mut matched_file_style = Style::default().fg(Color::Cyan).add_modifier(modifier);
            let mut padding_style = Style::default();

            if is_ignored {
                // Apply strikethrough and dimming to specific styles
                title_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
                category_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
                status_style = Style::default().fg(Color::DarkGray); // Dim the space for alignment
                matched_file_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
                padding_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);
            }
            
            // --- Create Spans --- 
            let mut spans = Vec::new();
            
            // Prefix (No style needed, inherits from item)
            let prefix = if is_focused && is_selected { "> " } else { "  " };
            spans.push(Span::raw(prefix)); 

            // Status Icon (Specific foreground color, inherits bg)
            if is_completed {
                spans.push(Span::styled("✓ ", Style::default().fg(Color::Green)));
            } else if is_ignored {
                spans.push(Span::styled("✗ ", Style::default().fg(Color::Red)));
            } else {
                spans.push(Span::styled("  ", status_style)); // Alignment space (inherits bg, gets dim+crossed out if ignored)
            }
            
            // Category Indicator (Uses calculated style)
            spans.push(Span::styled(type_indicator, category_style));
            
            // Padding (Uses calculated style)
            spans.push(Span::styled(padding, padding_style));
            
            // Title (Uses calculated style)
            spans.push(Span::styled(item.title.clone(), title_style));
            
            // Matched File (Uses calculated style)
            spans.push(Span::styled(matched_file_display, matched_file_style));

            // Apply background to the *entire* ListItem
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
