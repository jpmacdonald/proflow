//! `ProFlow` - Planning Center to `ProPresenter` workflow tool.

use error::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, EnableBracketedPaste, DisableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::{io, panic, time::Duration};

mod app;
mod bible;
mod config;
mod hymnal;
mod constants;
mod error;
mod input;
mod item_state;
mod lyrics;
mod planning_center;
mod propresenter;
mod services;
mod types;
mod ui;
mod utils;

use app::App;

// Helper function to ensure the terminal is cleaned up on exit
fn cleanup_terminal<B: Backend + std::io::Write>(terminal: &mut Terminal<B>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup better panic handling that cleans up terminal first
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // First disable raw mode
        let _ = disable_raw_mode();
        // Try to restore terminal to normal state
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        // Call the original panic handler
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run it
    // Note: App::new() loads its own config internally
    let app = App::new();
    let res = run_app(&mut terminal, app).await;

    // Restore terminal
    if let Err(e) = cleanup_terminal(&mut terminal) {
        eprintln!("Error cleaning up terminal: {e:?}");
    }

    if let Err(err) = res {
        eprintln!("{err:?}");
    }

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
    loop {
        app.handle_updates(); // Handle async updates first

        terminal.draw(|f| ui::draw(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let event::Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        } else {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        if app.should_quit() {
            break;
        }
    }
    Ok(()) // Return Ok(()) after loop breaks
} 