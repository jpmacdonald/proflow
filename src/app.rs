//! Application state and core logic.
//!
//! Contains the main `App` struct which holds all application state and
//! coordinates between the UI, input handling, and data services.

// Allow expect for compile-time constant regex patterns in static initializers
#![allow(clippy::expect_used)]

use crate::bible::{parse_scripture_ref, BibleService, BibleVersion, ScriptureHeader};
use crate::hymnal::{extract_hymn_number, HymnalService};
use crate::utils::file_index::{FileEntry, FileIndex};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use ratatui::widgets::ListState;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::editor::MIN_WRAP_COLUMN;

/// Maximum number of search results to display.
const MAX_SEARCH_RESULTS: usize = 20;

/// Channel buffer size for async task communication.
const CHANNEL_BUFFER_SIZE: usize = 10;
use crate::error::Result;
use crate::item_state::ItemStateStore;
use crate::planning_center::types::{Category, Item, Plan, Service};
use crate::planning_center::PlanningCenterClient;
use crate::services::search::{CompositeSearch, SearchStrategy};
use crate::planning_center::types::ItemId;

/// Messages sent from async tasks back to the main thread.
#[derive(Debug)]
pub enum AppUpdate {
    /// Services and plans have been fetched from Planning Center.
    DataLoaded(Result<(Vec<Service>, Vec<Plan>)>),
    /// Items for a specific plan have been fetched.
    ItemsLoaded(Result<Vec<Item>>),
}

/// Represents which screen the application is currently displaying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Initial splash screen shown at launch.
    Splash,
    /// Combined services and plans browser.
    ServiceList,
    /// Items list with file matching panel.
    ItemList,
    /// Text editor for slide content.
    Editor,
}

// Re-export SlideType from types module for backward compatibility
pub use crate::propresenter::SlideType;

// EditorState and VerseGroup are defined in crate::editor and re-exported here
// for backward compatibility with external consumers.
pub use crate::editor::EditorState;
pub use crate::editor::VerseGroup;

/// Root application state holding UI state, data caches, and service clients.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    /// Current screen/mode the application is displaying.
    pub mode: AppMode,
    /// Available Planning Center service types.
    pub services: Vec<Service>,
    /// Selection state for the service type list.
    pub service_list_state: ListState,
    /// ID of the currently selected service type.
    pub active_service_id: Option<String>,
    /// Plans loaded for the active service type.
    pub plans: Vec<Plan>,
    /// Selection state for the plan list.
    pub plan_list_state: ListState,
    /// Items in the currently selected plan.
    pub items: Vec<Item>,
    /// Consolidated per-item state (completion, ignored, matched file, editor, slide type).
    pub item_states: ItemStateStore,
    /// Selection state for the item list.
    pub item_list_state: ListState,
    /// Files matching the currently selected item.
    pub matching_files: Vec<FileEntry>,
    /// Selection state for the file match list.
    pub file_list_state: ListState,
    /// Whether file search mode is active in the command bar.
    pub file_search_active: bool,
    /// Current search query text in file search mode.
    pub file_search_query: String,
    /// State for the slide content editor.
    pub editor: EditorState,
    /// Available verse/section markers for lyrics.
    pub verse_groups: Vec<VerseGroup>,
    /// Buffer for the global `:command` being typed.
    pub global_command_buffer: String,
    /// Whether the global `:command` bar is active.
    pub is_global_command_mode: bool,
    /// Flag to signal the application should exit.
    should_quit: bool,
    /// Loaded application configuration.
    pub config: Config,
    /// Planning Center API client, if credentials are configured.
    pub pco_client: Option<PlanningCenterClient>,
    /// Sender for async task results.
    pub async_task_tx: mpsc::Sender<AppUpdate>,
    /// Receiver for async task results.
    async_task_rx: mpsc::Receiver<AppUpdate>,
    /// Whether an async data load is in progress.
    pub is_loading: bool,
    /// Error message to display in a modal overlay.
    pub error_message: Option<String>,
    /// Informational status message to display.
    pub status_message: Option<String>,
    /// Whether the help overlay is visible.
    pub show_help: bool,
    /// Path to the `ProPresenter` library directory.
    pub library_path: Option<PathBuf>,
    /// Whether initial data loading has been triggered.
    pub initialized: bool,
    /// Index of files in the library for fuzzy matching.
    pub file_index: Option<FileIndex>,
    /// Bible verse lookup service.
    pub bible_service: Option<BibleService>,
    /// Hymnal lookup service for curated `.txt` files.
    pub hymnal_service: Option<HymnalService>,
    /// Whether the Bible version picker overlay is shown.
    pub version_picker_active: bool,
    /// Currently selected index in the Bible version list.
    pub version_picker_selection: usize,
    /// Slide type for the item currently open in the editor.
    pub current_slide_type: SlideType,
    /// Selected index in the editor side pane list.
    pub editor_side_pane_idx: usize,
    /// Whether the editor side pane has keyboard focus.
    pub editor_side_pane_focused: bool,
    /// Parsed scripture header for display above editor content.
    pub current_scripture_header: Option<ScriptureHeader>,
    /// Number of uncompleted items pending playlist confirmation, if any.
    pub pending_playlist_confirmation: Option<usize>,
    /// Cache of `ProPresenter` templates for slide generation.
    pub template_cache: Option<crate::propresenter::template::TemplateCache>,
    /// Composite search strategy for file matching (liturgical + fuzzy).
    pub search: CompositeSearch,
}

/// Locate a subdirectory under the app's bundled data folder.
///
/// Search order:
/// 1. `$PROFLOW_DATA/<subdir>` (explicit override)
/// 2. `<data_dir>/proflow/<subdir>` (installed location via `dirs::data_dir`)
/// 3. `<exe_dir>/data/<subdir>` (next to the binary)
/// 4. `data/<subdir>` (cwd fallback, works during `cargo run`)
pub fn find_data_subdir(subdir: &str) -> PathBuf {
    // Explicit override
    if let Ok(base) = std::env::var("PROFLOW_DATA") {
        let p = PathBuf::from(base).join(subdir);
        if p.is_dir() {
            return p;
        }
    }

    // Platform data dir (~/Library/Application Support/proflow/ on macOS)
    if let Some(data) = dirs::data_dir() {
        let p = data.join("proflow").join(subdir);
        if p.is_dir() {
            return p;
        }
    }

    // Next to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("data").join(subdir);
            if p.is_dir() {
                return p;
            }
        }
    }

    // Fallback: cwd (works during cargo run)
    PathBuf::from("data").join(subdir)
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Creates a new `App` with configuration loaded from disk and services initialized.
    #[allow(clippy::too_many_lines)]
    pub fn new() -> Self {
        // Load configuration (fallback to default on error)
        let config = Config::load().unwrap_or_default();

        // Initialize Planning Center client if credentials are available
        let pco_client = config
            .has_planning_center_credentials()
            .then(|| PlanningCenterClient::new(&config));

        // Determine library path: env var > default location > config path
        let library_path = std::env::var("LIBRARY_DIR")
            .ok()
            .map(|s| PathBuf::from(shellexpand::tilde(&s).to_string()))
            .or_else(crate::utils::file_index::get_default_library_path)
            .or_else(|| {
                config.propresenter_path.as_ref().and_then(|pro_dir| {
                    let path = PathBuf::from(shellexpand::tilde(pro_dir).to_string())
                        .join("Libraries/Default");
                    path.exists().then_some(path)
                })
            });

        // Create the async channel
        let (async_task_tx, async_task_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

        // Extract hymnal service before config is moved into struct
        let hymnal_service = config
            .hymnal_path
            .as_ref()
            .map(|p| HymnalService::new(p.clone()));

        Self {
            mode: AppMode::Splash,
            services: Vec::new(),
            service_list_state: ListState::default(),
            active_service_id: None,
            plans: Vec::new(),
            plan_list_state: ListState::default(),
            items: Vec::new(),
            item_states: ItemStateStore::new(),
            item_list_state: ListState::default(),
            matching_files: Vec::new(),
            file_list_state: ListState::default(),
            editor: EditorState::default(),
            verse_groups: vec![
                VerseGroup {
                    name: "Verse".to_string(),
                    command: "v".to_string(),
                    color: Color::Blue,
                },
                VerseGroup {
                    name: "Chorus".to_string(),
                    command: "c".to_string(),
                    color: Color::Green,
                },
                VerseGroup {
                    name: "Bridge".to_string(),
                    command: "br".to_string(),
                    color: Color::Magenta,
                },
                VerseGroup {
                    name: "Tag".to_string(),
                    command: "t".to_string(),
                    color: Color::Cyan,
                },
                VerseGroup {
                    name: "Background".to_string(),
                    command: "bg".to_string(),
                    color: Color::Yellow,
                },
                VerseGroup {
                    name: "Interlude".to_string(),
                    command: "i".to_string(),
                    color: Color::Red,
                },
                VerseGroup {
                    name: "Refrain".to_string(),
                    command: "r".to_string(),
                    color: Color::LightBlue,
                },
                VerseGroup {
                    name: "Ending".to_string(),
                    command: "e".to_string(),
                    color: Color::LightGreen,
                },
                VerseGroup {
                    name: "Blank".to_string(),
                    command: "bl".to_string(),
                    color: Color::LightYellow,
                },
            ],
            global_command_buffer: String::new(),
            is_global_command_mode: false,
            should_quit: false,
            config,
            pco_client,
            async_task_tx,
            async_task_rx,
            is_loading: false,
            error_message: None,
            status_message: None,
            show_help: false,
            file_search_active: false,
            file_search_query: String::new(),
            library_path: library_path.clone(),
            initialized: false,
            file_index: None,
            bible_service: {
                let bible_path = find_data_subdir("bibles");
                Some(BibleService::new(bible_path))
            },
            hymnal_service,
            version_picker_active: false,
            version_picker_selection: 0, // Default to NRSVue
            current_slide_type: SlideType::Text,
            editor_side_pane_idx: 0,
            editor_side_pane_focused: false,
            current_scripture_header: None,
            pending_playlist_confirmation: None,
            template_cache: {
                let mut paths = Vec::new();
                if let Some(ref lib) = library_path {
                    paths.push(lib.clone());
                }
                paths.push(find_data_subdir("templates"));
                Some(crate::propresenter::template::ThemeCache::new(None, paths))
            },
            search: CompositeSearch::with_defaults(),
        }
    }

    /// Returns whether the application has been signalled to exit.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Signals the application to exit after the current frame.
    pub const fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Dispatches a keyboard event to the appropriate handler based on current mode.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Handle version picker if active
        if self.version_picker_active {
            self.handle_version_picker_input(key);
            return;
        }

        // First, check if help modal is shown
        if self.show_help {
            if key.code == KeyCode::Esc
                || key.code == KeyCode::F(1)
                || key.code == KeyCode::Char('?')
            {
                self.show_help = false;
            }
            return; // Don't process other keys while help is displayed
        }

        // Check if we need to dismiss an error or status message
        if self.error_message.is_some() {
            if key.code == KeyCode::Esc {
                self.error_message = None;
            }
            return; // Don't process other keys while error is displayed
        }
        if self.pending_playlist_confirmation.is_some() {
            match key.code {
                KeyCode::Char('y' | 'Y') => {
                    self.pending_playlist_confirmation = None;
                    self.status_message = None;
                    self.generate_playlist(true);
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                    self.pending_playlist_confirmation = None;
                    self.status_message = None;
                }
                _ => {}
            }
            return;
        }
        if self.status_message.is_some() {
            if key.code == KeyCode::Esc {
                self.status_message = None;
            }
            return;
        }

        // Global help shortcut (? or F1)
        if key.code == KeyCode::F(1)
            || (key.code == KeyCode::Char('?') && self.mode != AppMode::Editor)
        {
            self.show_help = true;
            return;
        }

        // Editor-local command mode with ':'
        if self.mode == AppMode::Editor && key.code == KeyCode::Char(':') {
            self.editor.is_command_mode = true;
            self.editor.command_buffer.clear();
            return;
        }

        // Then, handle global commands
        if self.is_global_command_mode {
            self.handle_global_command_input(key);
            return;
        }

        // Check for global shortcuts (non-editor)
        if key.code == KeyCode::Char(':') {
            self.is_global_command_mode = true;
            self.global_command_buffer.clear();
            return;
        }

        // Then handle mode-specific commands
        match self.mode {
            AppMode::Splash => self.handle_splash_input(key),
            AppMode::ServiceList => self.handle_service_list_input(key),
            AppMode::ItemList => self.handle_item_list_input(key),
            AppMode::Editor => self.handle_editor_input(key),
        }
    }

    fn handle_splash_input(&mut self, _key: KeyEvent) {
        // Initialize data when leaving splash screen
        if !self.initialized {
            // Initialize Planning Center data
            self.initialize_data();

            // Initialize file index if library path is available
            if let Some(lib_path) = &self.library_path {
                self.is_loading = true;
                match FileIndex::build(lib_path) {
                    Ok(index) => {
                        self.file_index = Some(index);
                        self.is_loading = false;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to index library: {e}"));
                        self.is_loading = false;
                    }
                }
            }

            // Load persisted item states from disk
            if let Some(dir) = ItemStateStore::cache_dir() {
                self.item_states = ItemStateStore::load(&dir);
            }

            self.initialized = true;
        }

        // Then move to the service list
        self.mode = AppMode::ServiceList;

        // Make sure loading state is still set when transitioning
        if self.services.is_empty() {
            self.is_loading = true;
        }
    }

    fn handle_global_command_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.is_global_command_mode = false;
                self.global_command_buffer.clear();
            }
            KeyCode::Enter => {
                self.execute_global_command();
                self.is_global_command_mode = false;
                self.global_command_buffer.clear();
            }
            KeyCode::Backspace => {
                self.global_command_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.global_command_buffer.push(c);
            }
            _ => {}
        }
    }

    /// Executes the command currently in the global command buffer.
    pub fn execute_global_command(&mut self) {
        match self.global_command_buffer.as_str() {
            "q" | "quit" => {
                // Signal that we want to exit cleanly
                self.quit();
            }
            "h" | "help" => {
                // TODO: Show help modal
            }
            "reload" | "refresh" => {
                // Reload data from the API
                self.retry_data_loading();
            }
            // Add other global commands here
            _ => {
                // If we don't recognize it as global, maybe it's a verse marker
                // Try to find a matching verse group
                if let Some(marker) = EditorState::parse_verse_marker(&self.global_command_buffer, &self.verse_groups) {
                    if self.mode == AppMode::Editor {
                        self.editor.insert_verse_marker(&marker);
                    }
                }
            }
        }
    }


    fn handle_service_list_input(&mut self, key: KeyEvent) {
        let service_focused = self.service_list_state.selected().is_some();

        let is_left_pane_focused = service_focused;

        if is_left_pane_focused {
            // --- Service List (Left Pane) Input ---
            let current_service_idx = self.service_list_state.selected().unwrap_or(0);
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if current_service_idx > 0 {
                        let new_idx = current_service_idx - 1;
                        self.service_list_state.select(Some(new_idx));
                        self.plan_list_state.select(None);
                        self.active_service_id = self.services.get(new_idx).map(|s| s.id.clone());
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if current_service_idx < self.services.len().saturating_sub(1) {
                        let new_idx = current_service_idx + 1;
                        self.service_list_state.select(Some(new_idx));
                        self.plan_list_state.select(None);
                        self.active_service_id = self.services.get(new_idx).map(|s| s.id.clone());
                    }
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab | KeyCode::Enter => {
                    if let Some(selected_service) = self.services.get(current_service_idx).cloned()
                    {
                        let has_plans = self
                            .plans
                            .iter()
                            .any(|p| p.service_id == selected_service.id);

                        if has_plans {
                            self.active_service_id = Some(selected_service.id);
                            self.plan_list_state.select(Some(0));
                            self.service_list_state.select(None);
                        }
                    }
                }
                _ => {}
            }
        } else {
            // --- Plan List (Right Pane) Input ---
            let num_displayed_plans = match &self.active_service_id {
                Some(id) => self.plans.iter().filter(|p| p.service_id == *id).count(),
                None => 0,
            };

            match key.code {
                KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                    self.plan_list_state.select(None);
                    if let Some(type_idx) = self
                        .active_service_id
                        .as_ref()
                        .and_then(|id| self.services.iter().position(|s| &s.id == id))
                    {
                        self.service_list_state.select(Some(type_idx));
                    } else if !self.services.is_empty() {
                        self.service_list_state.select(Some(0));
                        self.active_service_id = self.services.first().map(|s| s.id.clone());
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => match self.plan_list_state.selected() {
                    Some(selected) if selected > 0 => {
                        self.plan_list_state.select(Some(selected - 1));
                    }
                    Some(_) => {}
                    None => {
                        if num_displayed_plans > 0 {
                            self.plan_list_state.select(Some(num_displayed_plans - 1));
                        }
                    }
                },
                KeyCode::Down | KeyCode::Char('j') => match self.plan_list_state.selected() {
                    Some(selected) if selected < num_displayed_plans.saturating_sub(1) => {
                        self.plan_list_state.select(Some(selected + 1));
                    }
                    Some(_) => {}
                    None => {
                        if num_displayed_plans > 0 {
                            self.plan_list_state.select(Some(0));
                        }
                    }
                },
                KeyCode::Enter => {
                    if let Some(selected_idx_filtered) = self.plan_list_state.selected() {
                        if let Some(service_id) = &self.active_service_id {
                            if let Some(plan) = self
                                .plans
                                .iter()
                                .filter(|p| &p.service_id == service_id)
                                .nth(selected_idx_filtered)
                            {
                                let plan_id = plan.id.clone();
                                self.mode = AppMode::ItemList;
                                self.load_items_for_plan(&plan_id);
                            } else { /* ... error ... */
                            }
                        } else { /* ... error ... */
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_item_list_input(&mut self, key: KeyEvent) {
        // Handle file search mode (uses command bar)
        if self.file_search_active {
            self.handle_file_search_input(key);
            return;
        }

        let files_focused = self.file_list_state.selected().is_some();

        match key.code {
            KeyCode::Esc => {
                if files_focused {
                    self.file_list_state.select(None);
                } else {
                    self.mode = AppMode::ServiceList;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => match self.file_list_state.selected() {
                Some(selected) if selected > 0 => {
                    self.file_list_state.select(Some(selected - 1));
                }
                None => match self.item_list_state.selected() {
                    Some(selected) if selected > 0 => {
                        self.item_list_state.select(Some(selected - 1));
                        self.update_matching_files();
                    }
                    _ => {}
                },
                _ => {}
            },
            KeyCode::Down | KeyCode::Char('j') => match self.file_list_state.selected() {
                Some(selected) if selected < self.matching_files.len().saturating_sub(1) => {
                    self.file_list_state.select(Some(selected + 1));
                }
                None => match self.item_list_state.selected() {
                    Some(selected) if selected < self.items.len().saturating_sub(1) => {
                        self.item_list_state.select(Some(selected + 1));
                        self.update_matching_files();
                    }
                    _ => {}
                },
                _ => {}
            },
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                if !files_focused && !self.matching_files.is_empty() {
                    self.file_list_state.select(Some(0));
                } else if files_focused {
                    self.file_list_state.select(None);
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                self.file_list_state.select(None);
            }
            KeyCode::Char('/') => {
                // Activate file search mode (like k9s)
                self.file_search_active = true;
                self.file_search_query.clear();
            }
            KeyCode::Delete | KeyCode::Backspace | KeyCode::Char(' ') => {
                // Delete/Backspace/Space = toggle ignore (won't do) for current item
                if !files_focused {
                    if let Some(selected_idx) = self.item_list_state.selected() {
                        if let Some(item) = self.items.get(selected_idx) {
                            let item_id = ItemId::new(&item.id);
                            let currently_ignored = self.item_states.is_ignored(&item_id);
                            let new_ignored = !currently_ignored;
                            self.item_states.set_ignored(&item_id, new_ignored);

                            if new_ignored {
                                self.item_states.set_completed(&item_id, false);
                            }

                            self.persist_item_states();

                            if let Some(next_idx) = self.find_next_uncompleted_item(selected_idx) {
                                self.item_list_state.select(Some(next_idx));
                                self.update_matching_files();
                            }
                        }
                    }
                }
            }
            KeyCode::Enter => {
                if files_focused {
                    self.select_file_for_item();
                } else if !self.matching_files.is_empty() {
                    self.file_list_state.select(Some(0));
                }
            }
            KeyCode::Char('e' | 'c') if !files_focused => {
                // Edit key: open editor
                // - If item has matched .pro file → load its content
                // - If item has saved editor state → restore it
                // - Otherwise → create new with lyrics if available
                self.open_editor_for_item();
            }
            KeyCode::Char('g') if !files_focused => {
                self.try_generate_playlist();
            }
            KeyCode::Char('t') if !files_focused => {
                // Cycle slide type for current item
                if let Some(idx) = self.item_list_state.selected() {
                    if let Some(item) = self.items.get(idx) {
                        let item_id = ItemId::new(&item.id);
                        let current = self.get_slide_type_for_item(item);
                        let next = current.next();
                        self.item_states.set_slide_type(&item_id, Some(next));
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle input while in file search mode
    fn handle_file_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // Cancel search, restore original matches
                self.file_search_active = false;
                self.file_search_query.clear();
                self.update_matching_files();
            }
            KeyCode::Enter => {
                // Confirm search, keep results, exit search mode
                self.file_search_active = false;
                // Select file if one is highlighted
                if self.file_list_state.selected().is_some() {
                    self.select_file_for_item();
                    self.file_search_query.clear();
                    self.update_matching_files();
                }
            }
            KeyCode::Backspace => {
                self.file_search_query.pop();
                self.update_file_search();
            }
            KeyCode::Up => {
                if let Some(selected) = self.file_list_state.selected() {
                    if selected > 0 {
                        self.file_list_state.select(Some(selected - 1));
                    }
                }
            }
            KeyCode::Down => {
                if let Some(selected) = self.file_list_state.selected() {
                    if selected < self.matching_files.len().saturating_sub(1) {
                        self.file_list_state.select(Some(selected + 1));
                    }
                }
            }
            KeyCode::Char(c) => {
                self.file_search_query.push(c);
                self.update_file_search();
            }
            _ => {}
        }
    }

    /// Update file list based on search query (searches ALL files)
    fn update_file_search(&mut self) {
        if self.file_search_query.is_empty() {
            self.update_matching_files();
            return;
        }

        if let Some(index) = &self.file_index {
            self.matching_files = index.find_matches(&self.file_search_query, MAX_SEARCH_RESULTS);
            self.file_list_state
                .select(if self.matching_files.is_empty() {
                    None
                } else {
                    Some(0)
                });
        }
    }

    fn handle_editor_input(&mut self, key: KeyEvent) {
        if self.editor.is_command_mode {
            self.handle_editor_command_input(key);
        } else {
            self.handle_editor_normal_input(key);
        }

        // Ensure there's always an empty line at the end
        self.editor.ensure_empty_line_at_end();

        // Update the stored editor state in the store
        if let Some(item_idx) = self.item_list_state.selected() {
            if let Some(item) = self.items.get(item_idx) {
                let item_id = ItemId::new(&item.id);
                // Update the store with the current editor state
                self.item_states
                    .set_editor(&item_id, Some(self.editor.clone()));
            }
        }

        // Update scroll position to keep cursor in view
        // Scroll up if cursor moves above viewport
        if self.editor.cursor_y < self.editor.scroll_offset {
            self.editor.scroll_offset = self.editor.cursor_y;
        }
        // Scroll down only when cursor reaches bottom of viewport
        else if self.editor.cursor_y >= self.editor.scroll_offset + self.editor.viewport_height {
            self.editor.scroll_offset = self.editor.cursor_y - self.editor.viewport_height + 1;
        }
    }

    fn handle_editor_command_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.editor.is_command_mode = false;
                self.editor.command_buffer.clear();
            }
            KeyCode::Enter => {
                self.execute_editor_command();
                self.editor.is_command_mode = false;
                self.editor.command_buffer.clear();
            }
            KeyCode::Backspace => {
                self.editor.command_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.editor.command_buffer.push(c);
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_editor_normal_input(&mut self, key: KeyEvent) {
        // Tab to switch pane focus
        if key.code == KeyCode::Tab {
            self.editor_side_pane_focused = !self.editor_side_pane_focused;
            return;
        }

        // If side pane is focused, handle its keys
        if self.editor_side_pane_focused {
            self.handle_side_pane_input(key);
            return;
        }

        // Handle side pane shortcuts based on slide type (number keys work even when not focused)
        match (key.code, self.current_slide_type) {
            // Scripture mode: 1-4 to switch Bible versions
            (KeyCode::Char('1'), SlideType::Scripture) => {
                self.switch_bible_version(0);
                return;
            }
            (KeyCode::Char('2'), SlideType::Scripture) => {
                self.switch_bible_version(1);
                return;
            }
            (KeyCode::Char('3'), SlideType::Scripture) => {
                self.switch_bible_version(2);
                return;
            }
            (KeyCode::Char('4'), SlideType::Scripture) => {
                self.switch_bible_version(3);
                return;
            }
            _ => {}
        }

        match key.code {
            KeyCode::Esc => {
                // Clear selection when escaping
                self.editor.selection_active = false;

                // Check if editor has meaningful content (not just empty lines)
                let has_content = self
                    .editor
                    .content
                    .iter()
                    .any(|line| !line.trim().is_empty());

                if let Some(item_idx) = self.item_list_state.selected() {
                    if let Some(item) = self.items.get(item_idx) {
                        let item_id = ItemId::new(&item.id);

                        if has_content {
                            // Save editor state — this is a custom creation
                            self.item_states
                                .set_editor(&item_id, Some(self.editor.clone()));

                            // Clear any matched file — custom creation and file match are mutually exclusive
                            self.item_states.set_matched_file(&item_id, None);

                            // Mark as complete since we have content
                            self.item_states.set_completed(&item_id, true);
                        } else {
                            // No content — clear editor state
                            self.item_states.set_editor(&item_id, None);
                        }
                        self.persist_item_states();
                    }
                }
                self.mode = AppMode::ItemList;
            }
            // Select All (Cmd+A or Ctrl+A)
            KeyCode::Char('a') => {
                if key.modifiers.contains(KeyModifiers::META)
                    || key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    // Only proceed if we have content to select
                    if !self.editor.content.is_empty() {
                        // Set selection active
                        self.editor.selection_active = true;

                        // Set selection start to beginning of document
                        self.editor.selection_start_x = 0;
                        self.editor.selection_start_y = 0;

                        // Set cursor position to end of document
                        let last_line_idx = self.editor.content.len().saturating_sub(1);
                        self.editor.cursor_y = last_line_idx;

                        // Safely get the length of the last line
                        let last_line_len = self
                            .editor
                            .content
                            .get(last_line_idx)
                            .map_or(0, String::len);
                        self.editor.cursor_x = last_line_len;
                    }
                } else {
                    self.editor.insert_char('a');
                }
            }
            // Cut (Cmd+X or Ctrl+X)
            KeyCode::Char('x') => {
                if key.modifiers.contains(KeyModifiers::META)
                    || key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.editor.cut_selection();
                } else {
                    self.editor.insert_char('x');
                }
            }
            // Copy (Cmd+C or Ctrl+C)
            KeyCode::Char('c') => {
                if key.modifiers.contains(KeyModifiers::META)
                    || key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.editor.copy_selection();
                } else {
                    self.editor.insert_char('c');
                }
            }
            // Paste (Cmd+V or Ctrl+V)
            KeyCode::Char('v') => {
                if key.modifiers.contains(KeyModifiers::META)
                    || key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.editor.paste_from_clipboard();
                } else {
                    self.editor.insert_char('v');
                }
            }
            // Terminal-friendly keybindings for wrap guide
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => {
                if self.editor.wrap_column > MIN_WRAP_COLUMN {
                    self.editor.wrap_auto = false; // user is taking manual control
                    self.editor.wrap_column -= 1;
                }
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
                self.editor.wrap_auto = false; // user is taking manual control
                self.editor.wrap_column += 1;
            }
            // Handle keyboard selection with Shift+Arrow keys
            KeyCode::Left => {
                self.editor.handle_left_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Right => {
                self.editor.handle_right_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Up => {
                self.editor.handle_up_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Down => {
                self.editor.handle_down_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            // Regular character input - HJKL keys now work correctly
            KeyCode::Char(c) => {
                self.editor.insert_char(c);
            }
            KeyCode::Enter => {
                let current_line = &self.editor.content[self.editor.cursor_y];
                let remainder = if self.editor.cursor_x < current_line.len() {
                    current_line[self.editor.cursor_x..].to_string()
                } else {
                    String::new()
                };
                self.editor.content[self.editor.cursor_y] =
                    current_line[..self.editor.cursor_x].to_string();
                self.editor.cursor_y += 1;
                self.editor.content.insert(self.editor.cursor_y, remainder);
                self.editor.cursor_x = 0;
            }
            KeyCode::Backspace => {
                if self.editor.cursor_x > 0 {
                    let line = &mut self.editor.content[self.editor.cursor_y];
                    line.remove(self.editor.cursor_x - 1);
                    self.editor.cursor_x -= 1;
                } else if self.editor.cursor_y > 0 {
                    let current_line = self.editor.content.remove(self.editor.cursor_y);
                    self.editor.cursor_y -= 1;
                    self.editor.cursor_x = self.editor.content[self.editor.cursor_y].len();
                    self.editor.content[self.editor.cursor_y].push_str(&current_line);
                }
            }
            _ => {}
        }
    }


    fn execute_editor_command(&mut self) {
        let cmd = self.editor.command_buffer.clone();
        match cmd.as_str() {
            // "v1" => {
            //     self.insert_verse_marker("Verse 1");
            // }
            "split" => {
                if self.editor.cursor_y < self.editor.content.len() {
                    // Don't split the line itself, just insert an empty line at the cursor position
                    self.editor.cursor_y += 1;
                    self.editor.cursor_x = 0;
                    self.editor
                        .content
                        .insert(self.editor.cursor_y, String::new());
                }
            }
            "wrap" => {
                // Soft-wrap is always active, this is a no-op now
            }
            "wrap auto" => {
                self.editor.wrap_auto = true;
            }
            "q" | "quit" => {
                self.quit();
            }
            "export" | "save" => {
                self.export_editor_to_pro();
            }
            _ if cmd.starts_with("wrap ") => {
                if let Ok(col) = cmd[5..].parse::<usize>() {
                    self.editor.wrap_auto = false; // explicit manual wrap
                    self.editor.wrap_column = col.max(MIN_WRAP_COLUMN);
                }
            }
            _ if cmd.starts_with("export ") || cmd.starts_with("save ") => {
                // Export with custom filename
                let filename = cmd
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("presentation")
                    .to_string();
                self.export_editor_to_pro_with_name(&filename);
            }
            _ => {}
        }
    }

    /// Open editor for the currently selected item
    fn open_editor_for_item(&mut self) {
        let Some(idx) = self.item_list_state.selected() else {
            return;
        };
        let Some(item) = self.items.get(idx) else {
            return;
        };

        let item_id = item.id.clone();
        let title = item.title.clone();
        let item_id_typed = ItemId::new(&item_id);

        // Determine slide type
        let slide_type = self.get_slide_type_for_item(item);
        self.current_slide_type = slide_type;
        self.editor_side_pane_idx = 0;

        // Priority 1: Existing editor state (user's custom creation)
        if let Some(state) = self.item_states.get_editor(&item_id_typed) {
            self.editor = state.clone();
            self.mode = AppMode::Editor;
            return;
        }

        // Priority 2: Matched .pro file - extract its content
        if let Some(matched_path) = self.item_states.get_matched_file(&item_id_typed) {
            use crate::propresenter::extract::extract_text_from_pro;
            use std::path::Path;

            let path = Path::new(matched_path);
            if path.exists() && path.extension().is_some_and(|e| e == "pro") {
                match extract_text_from_pro(path) {
                    Ok(lines) => {
                        self.editor = EditorState {
                            content: if lines.is_empty() {
                                vec![String::new()]
                            } else {
                                lines
                            },
                            ..EditorState::default()
                        };
                        self.mode = AppMode::Editor;
                        return;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to load .pro: {e}"));
                        // Fall through to create new
                    }
                }
            }
        }

        // Priority 3: Scripture item - show version picker then load
        if slide_type == SlideType::Scripture {
            // Try to detect version from title
            if let Some(version) = BibleVersion::from_text(&title) {
                self.version_picker_selection = BibleVersion::all()
                    .iter()
                    .position(|v| *v == version)
                    .unwrap_or(0);
            }
            // Load scripture directly (version can be changed in side pane)
            self.load_scripture_into_editor();
            return;
        }

        // Priority 3.5: Hymnal lookup for lyrics items (curated .txt files)
        if slide_type == SlideType::Lyrics {
            let hymnal_result = self
                .hymnal_service
                .as_mut()
                .and_then(|h| h.lookup_from_title(&title));
            if let Some((_title, lines)) = hymnal_result {
                self.editor = EditorState {
                    content: lines,
                    ..EditorState::default()
                };
                self.mode = AppMode::Editor;
                return;
            }
        }

        // Priority 4: Song lyrics from Planning Center
        let lyrics = item.song.as_ref().and_then(|s| s.lyrics.as_ref());
        let mut new_state = EditorState::default();
        if let Some(lyrics) = lyrics {
            new_state.content = lyrics.lines().map(String::from).collect();
            if new_state.content.last().is_some_and(|l| !l.is_empty()) {
                new_state.content.push(String::new());
            }
        }

        self.editor = new_state;
        self.mode = AppMode::Editor;
    }

    /// Detect the slide type for an item based on category and title.
    fn detect_slide_type(category: Category, title: &str) -> SlideType {
        let title_lower = title.to_lowercase();

        // Check for explicit scripture indicators
        if title_lower.starts_with("scripture")
            || (title_lower.contains("scripture") && parse_scripture_ref(title).is_some())
            || parse_scripture_ref(title).is_some()
        {
            return SlideType::Scripture;
        }

        // Song category = Lyrics
        if matches!(category, Category::Song) {
            return SlideType::Lyrics;
        }

        // Title/nametag patterns
        if matches!(category, Category::Title) ||
           title_lower.contains("sermon") ||
           title_lower.contains("(robert)") || title_lower.contains("(hope)") ||  // Name patterns
           title_lower.starts_with("welcome")
        {
            return SlideType::Title;
        }

        // Graphic patterns
        if matches!(category, Category::Graphic)
            || title_lower.contains("pre-service")
            || title_lower.contains("preservice")
            || title_lower.contains("post-service")
            || title_lower.contains("postservice")
            || title_lower.contains("announcement")
            || (title_lower.contains("offertory") && !title_lower.contains(':'))
        {
            return SlideType::Graphic;
        }

        // Default to Text
        SlideType::Text
    }

    /// Get slide type for item (cached/overridden or detected)
    pub fn get_slide_type_for_item(&self, item: &Item) -> SlideType {
        let item_id = ItemId::new(&item.id);
        self.item_states
            .get_slide_type(&item_id)
            .unwrap_or_else(|| Self::detect_slide_type(item.category, &item.title))
    }

    /// Handle input when side pane is focused
    fn handle_side_pane_input(&mut self, key: KeyEvent) {
        match self.current_slide_type {
            SlideType::Scripture => {
                // Navigate versions
                let versions = BibleVersion::all();
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.version_picker_selection > 0 {
                            self.version_picker_selection -= 1;
                            self.reload_scripture();
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if self.version_picker_selection < versions.len() - 1 {
                            self.version_picker_selection += 1;
                            self.reload_scripture();
                        }
                    }
                    KeyCode::Enter => {
                        self.reload_scripture();
                        self.editor_side_pane_focused = false;
                    }
                    KeyCode::Esc => {
                        self.editor_side_pane_focused = false;
                    }
                    _ => {}
                }
            }
            _ => {
                // Navigate markers
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.editor_side_pane_idx > 0 {
                            self.editor_side_pane_idx -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if self.editor_side_pane_idx < self.verse_groups.len() - 1 {
                            self.editor_side_pane_idx += 1;
                        }
                    }
                    KeyCode::Enter => {
                        // Insert selected marker
                        if let Some(group) = self.verse_groups.get(self.editor_side_pane_idx) {
                            let marker = group.name.clone();
                            self.editor.insert_verse_marker(&marker);
                        }
                        self.editor_side_pane_focused = false;
                    }
                    KeyCode::Esc => {
                        self.editor_side_pane_focused = false;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Switch Bible version and reload scripture
    fn switch_bible_version(&mut self, version_idx: usize) {
        let versions = BibleVersion::all();
        if version_idx < versions.len() {
            self.version_picker_selection = version_idx;
            // Reload scripture with new version
            self.reload_scripture();
        }
    }

    /// Reload current scripture with selected version
    fn reload_scripture(&mut self) {
        let Some(idx) = self.item_list_state.selected() else {
            return;
        };
        let Some(item) = self.items.get(idx) else {
            return;
        };

        let title = &item.title;
        let version = BibleVersion::all()[self.version_picker_selection];

        // Parse scripture reference from title
        let Some(reference) = parse_scripture_ref(title) else {
            self.error_message = Some(format!("Could not parse: {title}"));
            return;
        };

        // Look up verses
        let Some(bible) = &mut self.bible_service else {
            self.error_message = Some("Bible data not available".to_string());
            return;
        };

        match bible.lookup(&reference, version) {
            Ok((header, lines)) => {
                self.current_scripture_header = Some(header);
                self.editor.content = lines;
                self.editor.cursor_x = 0;
                self.editor.cursor_y = 0;
                self.editor.scroll_offset = 0;
                self.editor.clamp_cursor();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed: {e}"));
            }
        }
    }

    /// Handle version picker input
    fn handle_version_picker_input(&mut self, key: KeyEvent) {
        let versions = BibleVersion::all();

        match key.code {
            KeyCode::Esc => {
                self.version_picker_active = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.version_picker_selection > 0 {
                    self.version_picker_selection -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.version_picker_selection < versions.len() - 1 {
                    self.version_picker_selection += 1;
                }
            }
            KeyCode::Enter => {
                self.load_scripture_into_editor();
                self.version_picker_active = false;
            }
            _ => {}
        }
    }

    /// Load scripture verses into the editor
    fn load_scripture_into_editor(&mut self) {
        let Some(idx) = self.item_list_state.selected() else {
            return;
        };
        let Some(item) = self.items.get(idx) else {
            return;
        };

        let title = &item.title;
        let version = BibleVersion::all()[self.version_picker_selection];

        // Parse scripture reference from title
        let Some(reference) = parse_scripture_ref(title) else {
            self.error_message = Some(format!("Could not parse scripture reference: {title}"));
            self.current_scripture_header = None;
            self.mode = AppMode::Editor;
            self.editor = EditorState::default();
            return;
        };

        // Look up verses
        let Some(bible) = &mut self.bible_service else {
            self.error_message = Some("Bible data not available".to_string());
            self.current_scripture_header = None;
            self.mode = AppMode::Editor;
            self.editor = EditorState::default();
            return;
        };

        match bible.lookup(&reference, version) {
            Ok((header, lines)) => {
                self.current_scripture_header = Some(header);
                self.editor = EditorState {
                    content: lines,
                    ..EditorState::default()
                };
                self.mode = AppMode::Editor;
                self.editor.clamp_cursor();
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load scripture: {e}"));
                self.current_scripture_header = None;
                self.mode = AppMode::Editor;
                self.editor = EditorState::default();
            }
        }
    }

    fn export_editor_to_pro(&mut self) {
        // Get the item title as the presentation name
        let name = self
            .get_current_item_title()
            .unwrap_or_else(|| "Untitled".to_string());
        self.export_editor_to_pro_with_name(&name);
    }

    fn export_editor_to_pro_with_name(&mut self, name: &str) {
        use crate::propresenter::serialize::write_presentation_file;
        use crate::propresenter::template::{
            build_presentation_from_template_with_options,
            DEFAULT_MAX_LINES_PER_SLIDE,
        };

        // Map slide type to template slide name
        let slide_name = match self.current_slide_type {
            SlideType::Scripture => "scripture",
            SlideType::Lyrics => "song",
            SlideType::Title | SlideType::Text | SlideType::Graphic => "info",
        };

        // Get template slide - require it to exist
        let Some(template_slide) = self
            .template_cache
            .as_mut()
            .and_then(|c| c.get(slide_name).cloned())
        else {
            self.error_message = Some(format!(
                "No template slide '{slide_name}' found! Configure a theme or add template files."
            ));
            return;
        };

        // Build presentation from template with auto-splitting
        let wrap_col = self.editor.wrap_column;

        // For scripture, include version in filename and add a title slide
        let (presentation_name, title_slide) = if self.current_slide_type == SlideType::Scripture {
            if let Some(ref header) = self.current_scripture_header {
                (
                    crate::propresenter::playlist::canonical_presentation_name(
                        &header.display(),
                        self.current_slide_type,
                    ),
                    Some(header.display()),
                )
            } else {
                (
                    crate::propresenter::playlist::canonical_presentation_name(name, self.current_slide_type),
                    None,
                )
            }
        } else {
            (
                crate::propresenter::playlist::canonical_presentation_name(name, self.current_slide_type),
                None,
            )
        };

        let segments = crate::propresenter::rtf::StyledSegment::from_plain(&self.editor.content);
        let Some(presentation) = build_presentation_from_template_with_options(
            &presentation_name,
            &template_slide,
            &segments,
            wrap_col,
            DEFAULT_MAX_LINES_PER_SLIDE,
            title_slide.as_deref(),
        ) else {
            self.error_message = Some("Failed to build presentation from template".to_string());
            return;
        };

        // Write to file
        let output_path = self.get_pro_output_path(&presentation_name);
        match write_presentation_file(&presentation, &output_path) {
            Ok(()) => {
                // Add to file index so it appears in suggestions immediately
                if let Some(ref mut index) = self.file_index {
                    index.add_entry(&output_path);
                }

                // Auto-match and complete the current item
                if let Some(item_idx) = self.item_list_state.selected() {
                    if let Some(item) = self.items.get(item_idx) {
                        let item_id = ItemId::new(&item.id);
                        self.item_states.set_matched_file(
                            &item_id,
                            Some(output_path.to_string_lossy().to_string()),
                        );
                        self.item_states.set_completed(&item_id, true);
                        self.persist_item_states();
                    }
                }

                self.update_matching_files();
                self.status_message = Some(format!("Exported: {}", output_path.display()));
            }
            Err(e) => {
                self.error_message = Some(format!("Export failed: {e}"));
            }
        }
    }

    fn get_current_item_title(&self) -> Option<String> {
        let item_idx = self.item_list_state.selected()?;
        self.items.get(item_idx).map(|i| i.title.clone())
    }

    fn get_pro_output_path(&self, name: &str) -> std::path::PathBuf {
        use crate::propresenter::playlist::sanitize_filename;

        let base_path = self
            .library_path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let safe_name = sanitize_filename(name, self.current_slide_type);
        let safe_name = if safe_name.is_empty() {
            "Untitled"
        } else {
            &safe_name
        };
        base_path.join(format!("{safe_name}.pro"))
    }

    fn load_items_for_plan(&mut self, plan_id: &str) {
        self.items.clear();
        self.item_list_state.select(None);
        self.matching_files.clear();
        self.file_list_state.select(None);

        let plan_id_owned = plan_id.to_string(); // Clone plan_id into an owned String for the task

        if let Some(client) = &self.pco_client {
            self.is_loading = true;
            let client_clone = client.clone();
            let tx_clone = self.async_task_tx.clone(); // Clone sender for the task

            // Spawn the async task using tokio::spawn
            tokio::spawn(async move {
                // Changed from self.runtime.spawn
                let result = client_clone.get_service_items(&plan_id_owned).await;
                // Send the result back to the main thread
                if let Err(_e) = tx_clone.send(AppUpdate::ItemsLoaded(result)).await {}
            });

            // Don't block here
        } else {
            self.error_message = Some(
                "Planning Center credentials are required. Set PCO_APP_ID and PCO_SECRET."
                    .to_string(),
            );
        }
    }


    #[allow(clippy::too_many_lines)]
    fn update_matching_files(&mut self) {
        self.matching_files.clear();
        self.file_list_state.select(None);

        let Some(selected_item_idx) = self.item_list_state.selected() else {
            return;
        };
        let Some(selected_item) = self.items.get(selected_item_idx).cloned() else {
            return;
        };

        // Extract title for searching
        let title = selected_item.title.clone();
        let item_id = selected_item.id.clone();

        // Build item-specific augmented search terms
        // Liturgical mappings and fuzzy matching are handled by CompositeSearch
        let mut augmented_terms: Vec<String> = Vec::new();

        // For scripture references, add variations with "v" instead of ":"
        if self.get_slide_type_for_item(&selected_item) == SlideType::Scripture
            && title.contains(':')
        {
            augmented_terms.push(title.replace(':', "v"));
        }

        // Extract any number references (like "#510") and add as search terms
        if let Some(number) = extract_hymn_number(&title) {
            augmented_terms.push(number.clone());
            augmented_terms.push(format!("#{number}"));
            augmented_terms.push(format!("Hymn {number}"));
            augmented_terms.push(format!("[Hymn] {number}"));

            // Look for significant words after the hymn number to use as additional terms
            if let Some(pos) = title.find(&number) {
                let after_number = title[pos + number.len()..].trim();
                if !after_number.is_empty() {
                    let key_words: Vec<&str> = after_number
                        .split_whitespace()
                        .filter(|word| {
                            word.len() > 3 && !["with", "from", "your", "thou"].contains(word)
                        })
                        .collect();

                    for word in key_words {
                        if !augmented_terms.contains(&word.to_string()) {
                            augmented_terms.push(word.to_string());
                        }
                    }
                }
            }
        }

        // Handle composite items with "and"
        if title.contains(" and ") {
            let parts: Vec<&str> = title.split(" and ").map(str::trim).collect();

            for part in parts {
                if part.len() > 3 && !augmented_terms.contains(&part.to_string()) {
                    augmented_terms.push(part.to_string());

                    let clean_part = part.trim_start_matches(|c: char| !c.is_alphanumeric());
                    if clean_part != part && clean_part.len() > 3 {
                        augmented_terms.push(clean_part.to_string());
                    }
                }
            }
        }

        // For composite terms with slashes like "Prayer/Lord's Prayer"
        if title.contains('/') {
            for part in title.split('/').map(str::trim) {
                if part.len() > 3 && !augmented_terms.contains(&part.to_string()) {
                    augmented_terms.push(part.to_string());
                }
            }
        }

        // For specific formats like "Offertory: O Love", add variations
        if title.contains(':') {
            let parts: Vec<&str> = title.split(':').map(str::trim).collect();
            if parts.len() >= 2 {
                if !augmented_terms.contains(&parts[0].to_string()) {
                    augmented_terms.push(parts[0].to_string());
                }
                if !augmented_terms.contains(&parts[1].to_string()) {
                    augmented_terms.push(parts[1].to_string());
                }
            }
        }

        // For songs, add song title and artist
        if let Some(song) = &selected_item.song {
            if song.title != title
                && !song.title.is_empty()
                && !augmented_terms.contains(&song.title)
            {
                augmented_terms.push(song.title.clone());
            }

            if let Some(author) = &song.author {
                if !author.is_empty() && author.len() > 3 && !augmented_terms.contains(author) {
                    augmented_terms.push(author.clone());
                }
            }
        }

        // Use the file index if available
        if let Some(index) = &self.file_index {
            let mut all_matches = Vec::new();
            let mut seen_paths = std::collections::HashSet::new();

            // Primary search: CompositeSearch handles liturgical mapping + fuzzy matching
            let primary_results: Vec<FileEntry> = self
                .search
                .find_matches(&title, &index.entries, 10)
                .into_iter()
                .cloned()
                .collect();
            for entry in primary_results {
                let path_str = entry.full_path.to_string_lossy().to_string();
                if seen_paths.insert(path_str) {
                    all_matches.push(entry);
                }
            }

            // Augmented search: item-specific terms use the file index's scoring
            for term in &augmented_terms {
                let matches = index.find_matches(term, 10);
                for entry in matches {
                    let path_str = entry.full_path.to_string_lossy().to_string();
                    if seen_paths.insert(path_str) {
                        all_matches.push(entry);
                    }
                }
            }

            // If we have a previous selection for this item, ensure it's always first
            let item_id_typed = ItemId::new(&item_id);
            if let Some(selected_path) = self.item_states.get_matched_file(&item_id_typed) {
                // Check if it's already in matches
                if let Some(selected_idx) = all_matches
                    .iter()
                    .position(|e| e.full_path.to_string_lossy() == selected_path)
                {
                    // Move to front
                    if selected_idx > 0 {
                        let selected_entry = all_matches.remove(selected_idx);
                        all_matches.insert(0, selected_entry);
                    }
                } else {
                    // Previous selection not in fuzzy results - find it in the index and add it
                    if let Some(entry) = index
                        .entries
                        .iter()
                        .find(|e| e.full_path.to_string_lossy() == *selected_path)
                    {
                        all_matches.insert(0, entry.clone());
                    }
                }
            }
            self.matching_files = all_matches;
        }
    }

    fn try_generate_playlist(&mut self) {
        // Count how many items are neither completed nor ignored
        let uncompleted_count = self
            .items
            .iter()
            .filter(|item| {
                let item_id = ItemId::new(&item.id);
                let is_completed = self.item_states.is_completed(&item_id);
                let is_ignored = self.item_states.is_ignored(&item_id);
                !is_completed && !is_ignored
            })
            .count();

        if uncompleted_count > 0 {
            self.pending_playlist_confirmation = Some(uncompleted_count);
            self.status_message = Some(format!(
                "Warning: {uncompleted_count} items are not matched/ignored. Continue? (y/n)"
            ));
            return;
        }

        self.generate_playlist(false);
    }

    fn generate_playlist(&mut self, allow_incomplete: bool) {
        use crate::propresenter::playlist::generate_playlist;

        let playlist_name = self
            .get_current_plan_title()
            .unwrap_or_else(|| "Service Playlist".to_string());

        let library_path = self.library_path.as_deref();

        // Pre-compute slide types to avoid borrowing self through the generation call.
        let slide_types: std::collections::HashMap<String, SlideType> = self
            .items
            .iter()
            .map(|item| (item.id.clone(), self.get_slide_type_for_item(item)))
            .collect();

        match generate_playlist(
            &self.items,
            &self.item_states,
            &mut self.template_cache,
            &slide_types,
            &playlist_name,
            library_path,
            allow_incomplete,
        ) {
            Ok(result) => {
                self.status_message = Some(format!(
                    "Playlist saved: {} ({} items)",
                    result.output_path.display(),
                    result.entry_count
                ));
            }
            Err(e) => {
                self.error_message = Some(e);
            }
        }
    }

    fn get_current_plan_title(&self) -> Option<String> {
        let plan_idx = self.plan_list_state.selected()?;
        let service_id = self.active_service_id.as_ref()?;

        let plan = self
            .plans
            .iter()
            .filter(|p| &p.service_id == service_id)
            .nth(plan_idx)?;

        let service_name = self
            .services
            .iter()
            .find(|s| &s.id == service_id)
            .map(|s| s.name.as_str());

        let date = plan.date.format("%B %e, %Y");
        let svc = service_name.unwrap_or("Service");

        Some(format!("{date} - {svc}"))
    }

    fn initialize_data(&mut self) {
        // Set loading state immediately
        self.is_loading = true;

        if let Some(client) = &self.pco_client {
            let client_clone = client.clone();
            let config_clone = self.config.clone();
            let tx_clone = self.async_task_tx.clone();

            // Spawn the async task using tokio::spawn
            tokio::spawn(async move {
                let result = client_clone
                    .get_upcoming_services(config_clone.days_ahead)
                    .await;
                if let Err(_e) = tx_clone.send(AppUpdate::DataLoaded(result)).await {}
            });
        } else {
            self.services.clear();
            self.plans.clear();
            self.initialize_selection_state();
            self.error_message = Some(
                "Planning Center credentials are required. Set PCO_APP_ID and PCO_SECRET."
                    .to_string(),
            );
            self.is_loading = false;
        }
    }

    // Helper function to set initial selection state after data is loaded
    fn initialize_selection_state(&mut self) {
        // eprintln!("[initialize_selection_state] Setting initial selection..."); // REMOVED
        if self.services.is_empty() {
            self.service_list_state.select(None);
            self.active_service_id = None;
        } else {
            self.service_list_state.select(Some(0));
            self.active_service_id = self.services.first().map(|s| s.id.clone());
        }
        self.plan_list_state.select(None);
    }

    /// Polls the async channel and applies any pending data updates.
    pub fn handle_updates(&mut self) {
        match self.async_task_rx.try_recv() {
            Ok(update) => {
                match update {
                    AppUpdate::DataLoaded(result) => {
                        self.is_loading = false;
                        match result {
                            Ok((services, plans)) => {
                                // Store the data from the API
                                self.services = services;
                                self.plans = plans;
                                self.initialize_selection_state();
                            }
                            Err(e) => {
                                self.error_message = Some(format!("Failed to load services: {e}"));
                            }
                        }
                    }
                    AppUpdate::ItemsLoaded(result) => {
                        self.is_loading = false; // Stop loading indicator
                        match result {
                            Ok(items) => {
                                self.items = items;

                                if !self.items.is_empty() {
                                    self.item_list_state.select(Some(0));
                                    self.update_matching_files();
                                }
                            }
                            Err(e) => {
                                self.error_message =
                                    Some(format!("Failed to load service items: {e}"));
                            }
                        }
                    }
                }
            }
            Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {}
        }
    }

    /// Re-initiates data loading after a previous failure.
    pub fn retry_data_loading(&mut self) {
        // Clear error message if present
        self.error_message = None;

        match self.mode {
            AppMode::ServiceList => {
                // Retry loading services and plans
                self.initialize_data();
            }
            AppMode::ItemList => {
                // If we have a selected plan, retry loading its items
                let plan_id = self.get_selected_plan_id();
                if let Some(id) = plan_id {
                    self.load_items_for_plan(&id);
                } else {
                    self.error_message = Some("No plan selected to reload".to_string());
                }
            }
            _ => {} // Other modes don't have data to reload
        }
    }

    // Helper method to get the currently selected plan ID
    fn get_selected_plan_id(&self) -> Option<String> {
        if let Some(selected_idx_filtered) = self.plan_list_state.selected() {
            if let Some(service_id) = &self.active_service_id {
                let filtered_plans: Vec<_> = self
                    .plans
                    .iter()
                    .filter(|p| &p.service_id == service_id)
                    .collect();

                if let Some(plan) = filtered_plans.get(selected_idx_filtered) {
                    return Some(plan.id.clone());
                }
            }
        }
        None
    }

    fn select_file_for_item(&mut self) {
        let Some(selected_file_idx) = self.file_list_state.selected() else {
            return;
        };
        let Some(selected_item_idx) = self.item_list_state.selected() else {
            return;
        };
        let Some(selected_item) = self.items.get(selected_item_idx) else {
            return;
        };
        let Some(selected_file) = self.matching_files.get(selected_file_idx) else {
            return;
        };

        let item_id = ItemId::new(&selected_item.id);
        let file_path = selected_file.full_path.to_string_lossy().to_string();

        // Clear editor state — file match and custom creation are mutually exclusive
        self.item_states.set_editor(&item_id, None);

        // Record the selection in our item state store
        self.item_states.set_matched_file(&item_id, Some(file_path));

        // Mark the item as completed
        self.item_states.set_completed(&item_id, true);
        self.persist_item_states();

        // Bump selection frequency in file index for future ranking
        if let Some(index) = &mut self.file_index {
            index.record_selection(&selected_file.full_path);
        }

        // Move to the next item if possible, otherwise deselect file list
        if let Some(next_idx) = self.find_next_uncompleted_item(selected_item_idx) {
            self.item_list_state.select(Some(next_idx));
            self.update_matching_files();
        } else {
            // No more uncompleted items - return focus to items list
            self.file_list_state.select(None);
        }
    }

    /// Persist item states to disk (no-op if cache directory is unavailable).
    fn persist_item_states(&self) {
        if let Some(dir) = ItemStateStore::cache_dir() {
            self.item_states.persist(&dir);
        }
    }

    // Helper to find next uncompleted item index
    fn find_next_uncompleted_item(&self, current_idx: usize) -> Option<usize> {
        ((current_idx + 1)..self.items.len()).find(|&i| {
            self.items.get(i).is_some_and(|item| {
                let item_id = ItemId::new(&item.id);
                !self.item_states.is_completed(&item_id) && !self.item_states.is_ignored(&item_id)
            })
        })
    }
}

