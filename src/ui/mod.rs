mod editor;
mod item_list;
mod service_list;

pub use editor::draw_editor;
pub use item_list::draw_item_list;
pub use service_list::draw_services;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect, Alignment},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Clear, Wrap},
    Frame,
};

use crate::app::{App, AppMode};

pub fn draw(f: &mut Frame, app: &mut App) {
    // Create the base layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3), // Command/status bar at bottom
        ])
        .split(f.size());

    // Draw the main content based on current mode
    match app.mode {
        AppMode::Splash => draw_splash(f, app, chunks[0]),
        AppMode::ServiceList => draw_services(f, app, chunks[0]),
        AppMode::ItemList => draw_item_list(f, app, chunks[0]),
        AppMode::Editor => draw_editor(f, app, chunks[0]),
    }

    // Draw loading indicator if needed
    if app.is_loading {
        draw_loading_indicator(f);
    }

    // Draw error message if present
    if let Some(error) = &app.error_message {
        draw_error_message(f, error);
    }

    // Draw help modal if shown
    if app.show_help {
        draw_help_modal(f, app);
    }

    // Draw command/status bar at the bottom (except in splash screen)
    if app.mode != AppMode::Splash {
        draw_command_bar(f, app, chunks[1]);
    } else {
        // Draw a simple press any key message
        let msg = "Press any key to continue...";
        
        // Make sure the area is large enough for the message
        if chunks[1].width >= msg.len() as u16 && chunks[1].height >= 3 {
            let width = msg.len() as u16;
            let x = (chunks[1].width.saturating_sub(width)) / 2;
            let y = chunks[1].top() + 1;
            
            let text_area = Rect {
                x: chunks[1].left() + x,
                y,
                width,
                height: 1,
            };
            
            let style = Style::default().fg(Color::Yellow);
            f.render_widget(Paragraph::new(msg).style(style), text_area);
        }
    }
}

// Draw the command bar (which shows help/status by default or command input when active)
fn draw_command_bar(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.is_global_command_mode { "Command" } else { "Commands/Status" };
    
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(title, Style::default().fg(Color::Yellow)));
    
    f.render_widget(block, area);
    
    // Calculate the inner area to render text with more padding
    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
        ])
        .margin(1)  // Add a margin of 1 to account for the border
        .split(area)[0];
    
    if app.is_global_command_mode {
        // Show command input with more left padding
        let command = Paragraph::new(format!(" :{}", app.global_command_buffer))
            .style(Style::default().fg(Color::Yellow));
        
        f.render_widget(command, inner_area);
    } else {
        // Show context-sensitive help/status with more left padding
        let help_text = match app.mode {
            AppMode::Splash => vec![], // No help text for splash screen
            AppMode::ServiceList => create_help_text(&[
                ("ESC", "Back"),
                ("Tab/Enter", "Switch focus"),
                (":reload", "Reload data"),
                (":q", "Quit"),
            ]),
            AppMode::ItemList => create_help_text(&[
                ("ESC", "Back"),
                ("Tab/Enter", "Switch focus"),
                ("c", "Create"),
                ("g", "Generate"),
                (":reload", "Reload data"),
                (":q", "Quit"),
            ]),
            AppMode::Editor => {
                let status = format!(
                    "Line: {}, Col: {} | Wrap: {}",
                    app.editor.cursor_y + 1,
                    app.editor.cursor_x + 1,
                    app.editor.wrap_column
                );

                let mut text = create_help_text(&[
                    ("ESC", "Back"),
                    ("Ctrl+C/X/V", "Copy/Cut/Paste"),
                    ("Ctrl+A", "Select All"),
                    ("Alt+←/→", "Adjust Wrap"),
                ]);
                
                // Add the status at the end
                text.push(Span::styled(status, Style::default().fg(Color::Gray)));
                
                text
            }
        };

        let status_bar = Paragraph::new(Line::from(help_text))
            .style(Style::default().fg(Color::Gray));

        f.render_widget(status_bar, inner_area);
    }
}

// Create consistently styled help text for command bar
pub fn create_help_text<'a>(commands: &[(&'a str, &'a str)]) -> Vec<Span<'a>> {
    let mut text = vec![Span::raw(" ")]; // Start with padding
    
    for (i, (key, description)) in commands.iter().enumerate() {
        // Add the key with bold styling
        text.push(Span::styled(*key, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        
        // Add the description
        text.push(Span::raw(format!(": {}", description)));
        
        // Add separator unless it's the last item
        if i < commands.len() - 1 {
            text.push(Span::raw(" | "));
        }
    }
    
    text
}

// Helper function to create a styled block with consistent appearance
pub fn create_titled_block<'a>(title: &'a str, is_focused: bool) -> Block<'a> {
    let title_style = if is_focused {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    
    Block::default()
        .title(Span::styled(title, title_style))
        .borders(Borders::ALL)
        .border_style(border_style)
}

fn draw_splash(f: &mut Frame, _app: &App, area: Rect) {
    // Define ASCII art logo for the app
    let logo = vec![
        r"  _____            ______ _                ",
        r" |  __ \          |  ____| |               ",
        r" | |__) | __ ___  | |__  | | _____      __ ",
        r" |  ___/ '__/ _ \ |  __| | |/ _ \ \ /\ / / ",
        r" | |   | | | (_) || |    | | (_) \ V  V /  ",
        r" |_|   |_|  \___/ |_|    |_|\___/ \_/\_/   ",
        r"                                           ",
        r"         Service Planning Made Easy         ",
        r"                                           ",
    ];
    
    // Use block to create a nice border around the splash
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightBlue))
        .title(Span::styled("ProFlow TUI", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    
    f.render_widget(block, area);
    
    // Calculate center position (accounting for border)
    let logo_height = logo.len() as u16;
    let logo_width = logo[0].len() as u16;
    
    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1)])
        .margin(1)  // Add a margin to account for the border
        .split(area)[0];
    
    let vertical_pad = (inner_area.height.saturating_sub(logo_height)) / 2;
    let horizontal_pad = (inner_area.width.saturating_sub(logo_width)) / 2;
    
    // Render each line of the logo
    for (i, line) in logo.iter().enumerate() {
        let y = inner_area.top() + vertical_pad + i as u16;
        if y >= inner_area.bottom() {
            break;
        }
        
        let text_area = Rect {
            x: inner_area.left() + horizontal_pad,
            y,
            width: line.len() as u16,
            height: 1,
        };
        
        let style = if i < 6 {
            // Logo itself is light blue
            Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)
        } else {
            // Tagline is yellow
            Style::default().fg(Color::Yellow)
        };
        
        f.render_widget(Paragraph::new(*line).style(style), text_area);
    }
    
    // Add version info at the bottom
    let version_text = "v0.1.0";
    
    // Make sure the area is large enough to display the version
    if area.width > (version_text.len() + 2) as u16 && area.height >= 2 {
        let version_area = Rect {
            x: area.right() - version_text.len() as u16 - 2,
            y: area.bottom() - 2,
            width: version_text.len() as u16,
            height: 1,
        };
        
        f.render_widget(
            Paragraph::new(version_text).style(Style::default().fg(Color::Gray)),
            version_area
        );
    }
}

// Draw a loading indicator overlay
fn draw_loading_indicator(f: &mut Frame) {
    let size = f.size();
    
    // Create a smaller centered box for the loading indicator
    let width = 22;
    let height = 3;
    
    let area = Rect {
        x: (size.width.saturating_sub(width)) / 2,
        y: (size.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    
    // Create a block with a border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    
    // Create loading text
    let text = Paragraph::new("Loading...")
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    
    f.render_widget(Clear, area); // Clear the area first
    f.render_widget(block, area);
    
    // Adjust area for inner text
    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1)])
        .margin(1) // Add a margin for the border
        .split(area)[0];
    
    f.render_widget(text, inner_area);
}

// Draw an error message overlay
fn draw_error_message(f: &mut Frame, message: &str) {
    let size = f.size();
    
    // Create a smaller centered box for the error message
    let width = 40.min(size.width.saturating_sub(4));
    let height = 5;
    
    let area = Rect {
        x: (size.width.saturating_sub(width)) / 2,
        y: (size.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    
    // Create a block with a border
    let block = Block::default()
        .title(Span::styled("Error", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black));
    
    // Create error text with word wrapping
    let text = Paragraph::new(message)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    
    f.render_widget(Clear, area); // Clear the area first
    f.render_widget(block, area);
    
    // Adjust area for inner text
    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Space after title
            Constraint::Min(1),
            Constraint::Length(1), // Space for a "Press Esc to dismiss" hint
        ])
        .margin(1) // Add a margin for the border
        .split(area);
    
    f.render_widget(text, inner_area[1]);
    
    // Add "Press Esc to dismiss" hint
    let hint = Paragraph::new("Press Esc to dismiss")
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);
    
    f.render_widget(hint, inner_area[2]);
}

// Draw the help modal with keybindings
fn draw_help_modal(f: &mut Frame, app: &App) {
    let size = f.size();
    
    // Calculate modal dimensions
    let width = 60.min(size.width.saturating_sub(4));
    let height = 24.min(size.height.saturating_sub(4));
    
    let area = Rect {
        x: (size.width.saturating_sub(width)) / 2,
        y: (size.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    
    // Create the modal block
    let block = Block::default()
        .title(Span::styled(" Help - Keybindings ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    
    // Inner area for content
    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1)])
        .margin(1)
        .split(area)[0];
    
    // Build help content based on current mode
    let help_lines = build_help_content(app);
    
    let help_text: Vec<Line> = help_lines.iter()
        .map(|(key, desc, is_header)| {
            if *is_header {
                Line::from(vec![
                    Span::styled(*key, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(format!("{:>12}", key), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::raw("  "),
                    Span::styled(*desc, Style::default().fg(Color::White)),
                ])
            }
        })
        .collect();
    
    let paragraph = Paragraph::new(help_text)
        .wrap(Wrap { trim: true });
    
    f.render_widget(paragraph, inner_area);
}

// Build help content based on current mode
fn build_help_content(app: &App) -> Vec<(&'static str, &'static str, bool)> {
    let mut lines = vec![
        ("── Global ──", "", true),
        ("F1 / ?", "Show this help", false),
        (":", "Enter command mode", false),
        (":q / :quit", "Quit application", false),
        (":reload", "Reload data from API", false),
        ("Esc", "Go back / dismiss modal", false),
        ("", "", false),
    ];
    
    match app.mode {
        AppMode::ServiceList => {
            lines.extend([
                ("── Services & Plans ──", "", true),
                ("↑/↓ or j/k", "Navigate items", false),
                ("←/→ or h/l", "Switch between panes", false),
                ("Tab", "Switch focus", false),
                ("Enter", "Select plan", false),
            ]);
        }
        AppMode::ItemList => {
            lines.extend([
                ("── Items & Files ──", "", true),
                ("↑/↓ or j/k", "Navigate items", false),
                ("Tab / →", "Switch to file list", false),
                ("Enter", "Select file for item", false),
                ("Del / Backspace", "Toggle ignore item", false),
                ("c", "Open editor for item", false),
                ("g", "Generate playlist", false),
            ]);
        }
        AppMode::Editor => {
            lines.extend([
                ("── Editor ──", "", true),
                ("↑/↓/←/→", "Move cursor", false),
                ("Shift+Arrows", "Select text", false),
                ("Ctrl+A", "Select all", false),
                ("Ctrl+C/X/V", "Copy/Cut/Paste", false),
                ("Alt+←/→", "Adjust wrap column", false),
                ("", "", false),
                ("── Commands ──", "", true),
                (":v1, :v2...", "Insert verse marker", false),
                (":c, :c1...", "Insert chorus marker", false),
                (":br", "Insert bridge marker", false),
                (":split", "Split at cursor", false),
                (":wrap", "Apply word wrap", false),
                (":wrap N", "Set wrap column to N", false),
                (":export/:save", "Export as .pro file", false),
            ]);
        }
        AppMode::Splash => {
            lines.extend([
                ("── Splash ──", "", true),
                ("Any key", "Continue to app", false),
            ]);
        }
    }
    
    // Add dismiss hint at the end
    lines.push(("", "", false));
    lines.push(("Press Esc, F1 or ? to close", "", true));
    
    lines
}
