use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use chrono::{DateTime, Local};
use crate::app::App;
use crate::ui::create_titled_block;
// No need to import Plan if not used in type hints

pub fn draw_services(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(area);
    
    // --- Left Pane: Services --- 
    let selected_service_index = app.service_list_state.selected();
    let service_items: Vec<ListItem> = app.services
        .iter()
        .enumerate()
        .map(|(i, service)| {
            let is_selected = Some(i) == selected_service_index;
            let (prefix, text_style) = if is_selected {
                ("> ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            } else {
                ("  ", Style::default().fg(Color::White))
            };
            
            ListItem::new(Line::from(vec![
                Span::raw(prefix),
                Span::styled(&service.name, text_style),
            ]))
        })
        .collect();

    let services_is_focused = app.service_list_state.selected().is_some();
    let services_list_widget = List::new(service_items)
        .block(create_titled_block(
            if services_is_focused { "Services (focused)" } else { "Services" },
            services_is_focused
        ))
        .highlight_style(Style::default().bg(Color::Rgb(80, 80, 120)).add_modifier(Modifier::BOLD))
        .highlight_symbol("");

    f.render_stateful_widget(services_list_widget, chunks[0], &mut app.service_list_state);

    // --- Right Pane: Plans --- 
    let filter_service_id = if app.plan_list_state.selected().is_some() {
        // Plan list focused: Use the stored active service ID
        app.active_service_id.as_deref()
    } else {
        // Service list focused: Use the ID matching the selected index
        app.service_list_state.selected()
            .and_then(|idx| app.services.get(idx).map(|s| s.id.as_str()))
    };

    // Filter plans based on the determined service ID
    let plans_to_display: Vec<&crate::planning_center::types::Plan> = match filter_service_id {
        Some(id) => {
            app.plans.iter().filter(|p| p.service_id == id).collect()
        },
        None => Vec::new() // Show empty if no service effectively selected
    };

    let selected_plan_index_in_filtered_list = app.plan_list_state.selected();
    let plan_list_items: Vec<ListItem> = plans_to_display
        .iter()
        .enumerate()
        .map(|(i, plan)| {
            let is_selected = Some(i) == selected_plan_index_in_filtered_list;
            let (prefix, text_style) = if is_selected {
                ("> ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            } else {
                ("  ", Style::default().fg(Color::White))
            };

            let local_date = plan.date.with_timezone(&Local);
            let date_str = format_date(&local_date);
            let title_part = if plan.title.is_empty() { "Service" } else { &plan.title };
            let title_width = 30;
            let title_display = if title_part.len() > title_width {
                format!("{}...", &title_part[0..(title_width-3)])
            } else {
                format!("{:title_width$}", title_part, title_width=title_width)
            };
            
            ListItem::new(Line::from(vec![
                Span::raw(prefix),
                Span::styled(title_display, text_style),
                Span::styled(format!(" ({})", date_str), Style::default().fg(Color::Gray)),
            ]))
        })
        .collect();

    let plans_is_focused = app.plan_list_state.selected().is_some();
    let plans_list_widget = List::new(plan_list_items)
        .block(create_titled_block(
            if plans_is_focused { "Plans (focused)" } else { "Plans" },
            plans_is_focused
        ))
        .highlight_style(Style::default().bg(Color::Rgb(80, 80, 120)).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(plans_list_widget, chunks[1], &mut app.plan_list_state);
}

// Helper function to format a date nicely
fn format_date(date: &DateTime<Local>) -> String {
    // Today's date in local time zone
    let today = Local::now().date_naive();
    
    // The date to format in local time zone
    let date_naive = date.date_naive();
    
    // Calculate difference in days
    let days_diff = (date_naive - today).num_days();
    
    match days_diff {
        0 => format!("Today, {}", date.format("%b %d")),
        1 => format!("Tomorrow, {}", date.format("%b %d")),
        2..=6 => format!("{}, {}", date.format("%a"), date.format("%b %d")),
        _ => format!("{}, {}", date.format("%b %d"), date.format("%Y")),
    }
}
