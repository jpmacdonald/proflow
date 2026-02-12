//! Input handling abstractions.
//!
//! This module provides traits and types for handling keyboard input
//! in a modular way, allowing mode-specific handlers to be tested independently.

use crossterm::event::KeyEvent;

/// Result of processing an input event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputResult {
    /// The input was consumed and handled.
    Consumed,
    /// The input was ignored (not applicable to this handler).
    Ignored,
    /// The application should quit.
    Quit,
    /// The mode should change.
    ModeChange(AppMode),
    /// An error occurred (message to display).
    Error(String),
    /// A status message should be shown.
    Status(String),
}

/// Application modes (mirrors `app::AppMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// Initial splash screen.
    Splash,
    /// Service/plan selection view.
    ServiceList,
    /// Item list with file matching.
    ItemList,
    /// Text editor for creating slides.
    Editor,
}

impl From<crate::app::AppMode> for AppMode {
    fn from(mode: crate::app::AppMode) -> Self {
        match mode {
            crate::app::AppMode::Splash => Self::Splash,
            crate::app::AppMode::ServiceList => Self::ServiceList,
            crate::app::AppMode::ItemList => Self::ItemList,
            crate::app::AppMode::Editor => Self::Editor,
        }
    }
}

impl From<AppMode> for crate::app::AppMode {
    fn from(mode: AppMode) -> Self {
        match mode {
            AppMode::Splash => Self::Splash,
            AppMode::ServiceList => Self::ServiceList,
            AppMode::ItemList => Self::ItemList,
            AppMode::Editor => Self::Editor,
        }
    }
}

/// Context passed to input handlers.
///
/// This provides handlers with the information they need to process
/// input without directly accessing the full App state.
#[allow(clippy::struct_excessive_bools)]
pub struct InputContext<'a> {
    /// Current application mode.
    pub mode: AppMode,
    /// Whether help is currently shown.
    pub show_help: bool,
    /// Whether there's an error message displayed.
    pub has_error: bool,
    /// Whether there's a pending confirmation.
    pub has_confirmation: bool,
    /// Whether in command mode (global or editor).
    pub is_command_mode: bool,
    /// Current command buffer contents.
    pub command_buffer: &'a str,
}

/// Trait for handling keyboard input.
///
/// Implementations of this trait handle input for specific modes
/// or input contexts.
pub trait InputHandler {
    /// Handle a key event.
    ///
    /// # Arguments
    /// * `key` - The key event to handle
    /// * `ctx` - Context about the current application state
    ///
    /// # Returns
    /// The result of handling the input.
    fn handle(&mut self, key: KeyEvent, ctx: &InputContext<'_>) -> InputResult;

    /// Get the name of this handler (for debugging).
    fn name(&self) -> &'static str;
}

/// Handler for global shortcuts (help, quit).
#[derive(Debug, Default)]
pub struct GlobalHandler;

impl InputHandler for GlobalHandler {
    fn handle(&mut self, key: KeyEvent, ctx: &InputContext<'_>) -> InputResult {
        use crossterm::event::KeyCode;

        // F1 or ? shows help (except in editor mode for ?)
        if key.code == KeyCode::F(1) {
            return InputResult::Status("Help".to_string());
        }

        if key.code == KeyCode::Char('?') && ctx.mode != AppMode::Editor {
            return InputResult::Status("Help".to_string());
        }

        InputResult::Ignored
    }

    fn name(&self) -> &'static str {
        "GlobalHandler"
    }
}

/// Handler for the splash screen.
#[derive(Debug, Default)]
pub struct SplashHandler;

impl InputHandler for SplashHandler {
    fn handle(&mut self, _key: KeyEvent, _ctx: &InputContext<'_>) -> InputResult {
        // Any key dismisses the splash screen
        InputResult::ModeChange(AppMode::ServiceList)
    }

    fn name(&self) -> &'static str {
        "SplashHandler"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn make_context(mode: AppMode) -> InputContext<'static> {
        InputContext {
            mode,
            show_help: false,
            has_error: false,
            has_confirmation: false,
            is_command_mode: false,
            command_buffer: "",
        }
    }

    #[test]
    fn test_splash_handler_any_key() {
        let mut handler = SplashHandler;
        let ctx = make_context(AppMode::Splash);
        let result = handler.handle(make_key(KeyCode::Enter), &ctx);

        assert_eq!(result, InputResult::ModeChange(AppMode::ServiceList));
    }

    #[test]
    fn test_global_handler_f1() {
        let mut handler = GlobalHandler;
        let ctx = make_context(AppMode::ItemList);
        let result = handler.handle(make_key(KeyCode::F(1)), &ctx);

        matches!(result, InputResult::Status(_));
    }

    #[test]
    fn test_global_handler_question_mark_not_in_editor() {
        let mut handler = GlobalHandler;
        let ctx = make_context(AppMode::ItemList);
        let result = handler.handle(make_key(KeyCode::Char('?')), &ctx);

        matches!(result, InputResult::Status(_));
    }

    #[test]
    fn test_global_handler_question_mark_in_editor_ignored() {
        let mut handler = GlobalHandler;
        let ctx = make_context(AppMode::Editor);
        let result = handler.handle(make_key(KeyCode::Char('?')), &ctx);

        assert_eq!(result, InputResult::Ignored);
    }
}
