//! User interface components.
//!
//! Provides TUI widgets and drawing functions for the application's
//! terminal-based user interface using ratatui.

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

/// Render the full application UI to the terminal frame.
#[allow(clippy::cast_possible_truncation)]
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

    // Draw status/info modal (blocking)
    if let Some(status) = &app.status_message {
        draw_status_message(f, status);
        return;
    }
    // Draw error message if present (blocking)
    if let Some(error) = &app.error_message {
        draw_error_message(f, error);
        return;
    }

    // Draw help modal if shown
    if app.show_help {
        draw_help_modal(f, app);
    }

    // Draw version picker if active
    if app.version_picker_active {
        draw_version_picker(f, app);
    }

    // Draw command/status bar at the bottom (except in splash screen)
    if app.mode == AppMode::Splash {
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
    } else {
        draw_command_bar(f, app, chunks[1]);
    }
}

#[allow(clippy::cast_possible_truncation)]
fn draw_command_bar(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.is_global_command_mode { 
        "Command" 
    } else if app.file_search_active { 
        "Search Files" 
    } else { 
        "Commands/Status" 
    };
    
    let border_color = if app.file_search_active { Color::Cyan } else { Color::Yellow };
    
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, Style::default().fg(border_color)));
    
    f.render_widget(block, area);
    
    // Calculate the inner area to render text with more padding
    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
        ])
        .margin(1)  // Add a margin of 1 to account for the border
        .split(area)[0];
    
    if app.mode == AppMode::Editor && app.editor.is_command_mode {
        let command = Paragraph::new(format!(" :{}", app.editor.command_buffer))
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(command, inner_area);
        f.set_cursor(inner_area.left() + app.editor.command_buffer.len() as u16 + 2, inner_area.top());
    } else if app.file_search_active {
        // Show file search input
        let search = Paragraph::new(format!(" /{}", app.file_search_query))
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(search, inner_area);
    } else if app.is_global_command_mode {
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
                ("Enter", "Match"),
                ("e", "Edit"),
                ("t", "Type"),
                ("Space", "Skip"),
                ("g", "Generate"),
            ]),
            AppMode::Editor => {
                let status = format!(
                    "Ln {}, Col {} | Wrap: {}",
                    app.editor.cursor_y + 1,
                    app.editor.cursor_x + 1,
                    app.editor.wrap_column
                );

                // Context-sensitive hints based on slide type
                let hints: &[(&str, &str)] = match app.current_slide_type {
                    crate::app::SlideType::Scripture => &[
                        ("ESC", "Back"),
                        ("Tab", "Versions"),
                        (":wrap", "Word wrap"),
                        (":export", "Save"),
                    ],
                    _ => &[
                        ("ESC", "Back"),
                        ("Tab", "Markers"),
                        (":wrap", "Word wrap"),
                        (":export", "Save"),
                    ],
                };
                
                let mut text = create_help_text(hints);
                text.push(Span::styled(format!(" | {status}"), Style::default().fg(Color::Gray)));
                
                text
            }
        };

        let status_bar = Paragraph::new(Line::from(help_text))
            .style(Style::default().fg(Color::Gray));

        f.render_widget(status_bar, inner_area);
    }
}

/// Build styled help text spans from key-description pairs for the command bar.
pub fn create_help_text<'a>(commands: &[(&'a str, &'a str)]) -> Vec<Span<'a>> {
    let mut text = vec![Span::raw(" ")]; // Start with padding
    
    for (i, (key, description)) in commands.iter().enumerate() {
        // Add the key with bold styling
        text.push(Span::styled(*key, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        
        // Add the description
        text.push(Span::raw(format!(": {description}")));
        
        // Add separator unless it's the last item
        if i < commands.len() - 1 {
            text.push(Span::raw(" | "));
        }
    }
    
    text
}

/// Create a bordered block with a title, highlighted when focused.
pub fn create_titled_block(title: &str, is_focused: bool) -> Block<'_> {
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

#[allow(clippy::cast_possible_truncation)]
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

#[allow(clippy::cast_possible_truncation)]
fn draw_status_message(f: &mut Frame, message: &str) {
    use unicode_width::UnicodeWidthStr;
    let size = f.size();

    // Calculate box width (max 80% of screen, min 50)
    let max_width = (size.width as usize * 80) / 100;
    let width = message.width()
        .saturating_add(6)
        .min(max_width)
        .max(50) as u16;
    
    // Calculate how many lines the message will need when wrapped
    let inner_width = width.saturating_sub(4) as usize; // account for borders + margin
    let msg_lines = (message.width() + inner_width - 1) / inner_width.max(1);
    let height = (msg_lines as u16 + 4).min(size.height.saturating_sub(4)); // +4 for borders, hint, padding
    
    let area = Rect {
        x: (size.width.saturating_sub(width)) / 2,
        y: (size.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    
    let block = Block::default()
        .title(Span::styled("Info", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    
    let text = Paragraph::new(message)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    
    let inner_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),      // message (flexible)
            Constraint::Length(1),   // hint
        ])
        .margin(1)
        .split(area);
    
    f.render_widget(text, inner_area[0]);
    
    let hint = Paragraph::new("Press Esc to dismiss")
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);
    
    f.render_widget(hint, inner_area[1]);
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
                    Span::styled(format!("{key:>12}"), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
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
                ("/", "Search all files", false),
                ("Enter", "Select file for item", false),
                ("Space", "Toggle ignore item", false),
                ("e", "Edit item (load .pro or create)", false),
                ("t", "Cycle slide type", false),
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
                ("── Scripture ──", "", true),
                ("1-4", "Switch Bible version", false),
                ("", "", false),
                ("── Commands ──", "", true),
                (":v1, :v2...", "Insert verse marker", false),
                (":c, :c1...", "Insert chorus marker", false),
                (":br", "Insert bridge marker", false),
                (":wrap", "Apply word wrap", false),
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

// Draw the Bible version picker modal
fn draw_version_picker(f: &mut Frame, app: &App) {
    use crate::bible::BibleVersion;
    
    let size = f.size();
    
    // Calculate modal dimensions
    let width = 30.min(size.width.saturating_sub(4));
    let height = 10.min(size.height.saturating_sub(4));
    
    let area = Rect {
        x: (size.width.saturating_sub(width)) / 2,
        y: (size.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    
    // Create the modal block
    let block = Block::default()
        .title(Span::styled(" Select Bible Version ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
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
    
    // Build version list
    let versions = BibleVersion::all();
    let version_lines: Vec<Line> = versions.iter()
        .enumerate()
        .map(|(i, v)| {
            let is_selected = i == app.version_picker_selection;
            let prefix = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(format!("{prefix}{}", v.name()), style))
        })
        .collect();
    
    let paragraph = Paragraph::new(version_lines);
    f.render_widget(paragraph, inner_area);
}
