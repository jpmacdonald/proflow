use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use arboard::Clipboard;
use ratatui::style::Color;
use std::path::PathBuf;
use crate::utils::file_matcher::{find_matches_for_items, FileIndex, FileEntry};
use tokio::sync::mpsc;
use std::collections::HashMap;

use crate::config::Config;
use crate::error::Result;
use crate::planning_center::PlanningCenterClient;
use crate::planning_center::types::{Service, Plan, Item, Category};

// Define messages for async communication
#[derive(Debug)]
pub enum AppUpdate {
    DataLoaded(Result<(Vec<Service>, Vec<Plan>)>),
    ItemsLoaded(Result<Vec<Item>>),
    // Can add more message types later if needed
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Splash,       // Initial splash screen
    ServiceList,  // Combined Services and Plans view
    ItemList,     // Items and Files view
    Editor,       // Editor view
}

#[derive(Debug, Clone)]
pub struct EditorState {
    pub content: Vec<String>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub scroll_offset: usize,
    pub wrap_column: usize,
    pub command_buffer: String,
    pub is_command_mode: bool,
    pub viewport_height: usize,
    pub selection_active: bool,
    pub selection_start_x: usize,
    pub selection_start_y: usize,
}

// Define a struct to represent a verse group
#[derive(Debug, Clone)]
pub struct VerseGroup {
    pub name: String,       // Full name (e.g., "Verse 1")
    pub command: String,    // Command to create it (e.g., "v1")
    pub color: Color,       // Display color
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            content: vec![String::new(), String::new()],
            cursor_x: 0,
            cursor_y: 0,
            scroll_offset: 0,
            wrap_column: 80,
            command_buffer: String::new(),
            is_command_mode: false,
            viewport_height: 20, // Default value until UI updates it
            selection_active: false,
            selection_start_x: 0,
            selection_start_y: 0,
        }
    }
}

pub struct App {
    pub mode: AppMode,
    pub services: Vec<Service>,
    pub service_list_state: ListState,
    pub active_service_id: Option<String>,
    pub plans: Vec<Plan>, 
    pub plan_list_state: ListState,
    pub items: Vec<Item>,
    pub item_completion: HashMap<String, bool>,
    pub item_ignored: HashMap<String, bool>,
    pub item_matched_file: HashMap<String, Option<String>>,
    pub item_editor_state: HashMap<String, Option<EditorState>>,
    pub item_list_state: ListState,
    pub matching_files: Vec<FileEntry>,
    pub file_list_state: ListState,
    pub editor: EditorState,
    pub verse_groups: Vec<VerseGroup>,
    pub global_command_buffer: String,
    pub is_global_command_mode: bool,
    pub should_quit: bool,
    pub config: Config,
    pub pco_client: Option<PlanningCenterClient>,
    pub async_task_tx: mpsc::Sender<AppUpdate>,
    async_task_rx: mpsc::Receiver<AppUpdate>,
    pub is_loading: bool,
    pub error_message: Option<String>,
    pub library_path: Option<PathBuf>,
    pub initialized: bool,
    pub file_index: Option<FileIndex>,
}

impl App {
    pub fn new() -> Self {
        // eprintln!("[App::new] Starting App initialization..."); // REMOVED
        // Initialize Tokio runtime for async operations - REMOVED manual creation
        // let runtime = Runtime::new().expect("Failed to create Tokio runtime");
        // eprintln!("[App::new] Tokio runtime created successfully.");
        
        // Load configuration (fallback to default on error)
        let config = Config::load().unwrap_or_default();
        
        // Initialize Planning Center client if credentials are available
        let pco_client = config.has_planning_center_credentials()
            .then(|| PlanningCenterClient::new(&config));
        
        // Determine library path: env var > default location > config path
        let library_path = std::env::var("LIBRARY_DIR").ok()
            .map(|s| PathBuf::from(shellexpand::tilde(&s).to_string()))
            .or_else(crate::utils::file_matcher::get_default_library_path)
            .or_else(|| {
                config.propresenter_path.as_ref().and_then(|pro_dir| {
                    let path = PathBuf::from(shellexpand::tilde(pro_dir).to_string())
                        .join("Libraries/Default");
                    path.exists().then_some(path)
                })
            });

        // Create the async channel
        let (async_task_tx, async_task_rx) = mpsc::channel(10); // Channel with buffer size 10

        let app = Self {
            mode: AppMode::Splash, 
            services: Vec::new(),
            service_list_state: ListState::default(),
            active_service_id: None,
            plans: Vec::new(),
            plan_list_state: ListState::default(),
            items: Vec::new(),
            item_completion: HashMap::new(),
            item_ignored: HashMap::new(),
            item_matched_file: HashMap::new(),
            item_editor_state: HashMap::new(),
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
            library_path,
            initialized: false,
            file_index: None,
        };
        
        // Don't initialize data right away - wait for splash screen to be dismissed
        
        app
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // First, check if we need to dismiss an error message
        if self.error_message.is_some() {
            if key.code == KeyCode::Esc {
                self.error_message = None;
            }
            return; // Don't process other keys while error is displayed
        }

        // Then, handle global commands
        if self.is_global_command_mode {
            self.handle_global_command_input(key);
            return;
        }

        // Check for global shortcuts
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
                    },
                    Err(e) => {
                        self.error_message = Some(format!("Failed to index library: {}", e));
                        self.is_loading = false;
                    }
                }
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
                if let Some(marker) = self.parse_verse_marker(&self.global_command_buffer) {
                    if self.mode == AppMode::Editor {
                        self.insert_verse_marker(&marker);
                    }
                }
            }
        }
    }

    fn parse_verse_marker(&self, command: &str) -> Option<String> {
        for group in &self.verse_groups {
            // Check if command starts with a verse group command
            if command.starts_with(&group.command) {
                let remainder = &command[group.command.len()..];
                
                // If there's nothing after the command, just use the base name
                if remainder.is_empty() {
                    return Some(group.name.clone());
                }
                
                // Otherwise, try to parse a number
                if let Ok(num) = remainder.parse::<u32>() {
                    return Some(format!("{} {}", group.name, num));
                }
            }
        }
        None
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
                     if let Some(selected_service) = self.services.get(current_service_idx).cloned() {
                        let plans_for_type: Vec<_> = self.plans.iter()
                            .filter(|p| p.service_id == selected_service.id)
                            .collect();
                        
                        if !plans_for_type.is_empty() {
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
                    if let Some(type_idx) = self.active_service_id.as_ref()
                        .and_then(|id| self.services.iter().position(|s| &s.id == id)) {
                        self.service_list_state.select(Some(type_idx));
                    } else if !self.services.is_empty() {
                        self.service_list_state.select(Some(0));
                        self.active_service_id = self.services.get(0).map(|s| s.id.clone());
                    }
                }
            KeyCode::Up | KeyCode::Char('k') => {
                    match self.plan_list_state.selected() {
                    Some(selected) if selected > 0 => {
                            self.plan_list_state.select(Some(selected - 1));
                        }
                        Some(_) => {}
                        None => {
                           if num_displayed_plans > 0 {
                                self.plan_list_state.select(Some(num_displayed_plans - 1));
                           }
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    match self.plan_list_state.selected() {
                        Some(selected) if selected < num_displayed_plans.saturating_sub(1) => {
                            self.plan_list_state.select(Some(selected + 1));
                        }
                         Some(_) => {}
                         None => {
                            if num_displayed_plans > 0 {
                        self.plan_list_state.select(Some(0));
                    }
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(selected_idx_filtered) = self.plan_list_state.selected() {
                        if let Some(service_id) = &self.active_service_id {
                             if let Some(plan) = self.plans.iter()
                                .filter(|p| &p.service_id == service_id)
                                .nth(selected_idx_filtered) {
                                     let plan_id = plan.id.clone(); 
                                     self.mode = AppMode::ItemList;
                                     self.load_items_for_plan(&plan_id);
                             } else { /* ... error ... */ }
                        } else { /* ... error ... */ }
                }
            }
            _ => {}
            }
        }
    }

    fn handle_item_list_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::ServiceList;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match self.file_list_state.selected() {
                    Some(selected) if selected > 0 => {
                        // File list is focused
                        self.file_list_state.select(Some(selected - 1));
                    }
                    None => {
                        // Item list is focused
                        match self.item_list_state.selected() {
                            Some(selected) if selected > 0 => {
                                self.item_list_state.select(Some(selected - 1));
                                self.update_matching_files(); // Update matches for new selection
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.file_list_state.selected() {
                    Some(selected) if selected < self.matching_files.len().saturating_sub(1) => {
                        // File list is focused
                        self.file_list_state.select(Some(selected + 1));
                    }
                    None => {
                        // Item list is focused
                        match self.item_list_state.selected() {
                            Some(selected) if selected < self.items.len().saturating_sub(1) => {
                                self.item_list_state.select(Some(selected + 1));
                                self.update_matching_files(); // Update matches for new selection
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                // Switch focus between item list and file list
                match (self.file_list_state.selected(), !self.matching_files.is_empty()) {
                    (None, true) => {
                        // Focus file list
                        self.file_list_state.select(Some(0));
                    }
                    (Some(_), _) => {
                        // Return focus to item list
                    self.file_list_state.select(None);
                    }
                    _ => {}
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                // Return focus to item list
                self.file_list_state.select(None);
            }
            KeyCode::Delete | KeyCode::Backspace => {
                // Toggle ignored status on the selected item
                if self.file_list_state.selected().is_none() {
                    // Only if item list is focused
                    if let Some(selected_idx) = self.item_list_state.selected() {
                        if let Some(item) = self.items.get(selected_idx) {
                            let item_id = item.id.clone();
                            
                            // Toggle the ignored status
                            let currently_ignored = *self.item_ignored.get(&item_id).unwrap_or(&false);
                            self.item_ignored.insert(item_id.clone(), !currently_ignored);
                            
                            // If we're marking as ignored, make sure it's not also marked as completed
                            if !currently_ignored {
                                self.item_completion.insert(item_id, false);
                            }
                            
                            // Move to the next item if possible
                            if let Some(next_idx) = self.find_next_uncompleted_item(selected_idx) {
                                self.item_list_state.select(Some(next_idx));
                                        self.update_matching_files();
                                    }
                                }
                            }
                        }
                    }
            KeyCode::Enter => {
                // If file list is focused, select file for item
                if self.file_list_state.selected().is_some() {
                    self.select_file_for_item();
                } else {
                    // If item list is focused, switch focus to file list if files exist
                    if !self.matching_files.is_empty() {
                            self.file_list_state.select(Some(0));
                    }
                }
            }
            KeyCode::Char('c') => {
                // Create/edit mode - open editor for selected item
                if let Some(idx) = self.item_list_state.selected() {
                    if let Some(item) = self.items.get(idx) {
                        let item_id = item.id.clone();
                        
                        // Use existing editor state or create new one
                        let state = self.item_editor_state.get(&item_id)
                            .and_then(|s| s.clone())
                            .unwrap_or_else(|| {
                                let mut new_state = EditorState::default();
                                
                                // Initialize with lyrics if available
                                if let Some(lyrics) = item.song.as_ref().and_then(|s| s.lyrics.as_ref()) {
                                    new_state.content = lyrics.lines().map(String::from).collect();
                                    // Ensure trailing newline
                                    if new_state.content.last().map_or(false, |l| !l.is_empty()) {
                                        new_state.content.push(String::new());
                                    }
                                }
                                
                                self.item_editor_state.insert(item_id, Some(new_state.clone()));
                                new_state
                            });
                        
                        self.editor = state;
                        self.mode = AppMode::Editor;
                    }
                }
            }
            KeyCode::Char('g') => {
                self.try_generate_playlist();
            }
            _ => {}
        }
    }

    fn handle_editor_input(&mut self, key: KeyEvent) {
        if self.editor.is_command_mode {
            self.handle_editor_command_input(key);
        } else {
            self.handle_editor_normal_input(key);
        }

        // Ensure there's always an empty line at the end
        self.ensure_empty_line_at_end();

        // Update the stored editor state in the map
        if let Some(item_idx) = self.item_list_state.selected() {
            if let Some(item) = self.items.get(item_idx) {
                let item_id = item.id.clone();
                // Update the map with the current editor state
                self.item_editor_state.insert(item_id, Some(self.editor.clone()));
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

    fn handle_editor_normal_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // Clear selection when escaping
                self.editor.selection_active = false;
                self.mode = AppMode::ItemList;
            }
            // Select All (Cmd+A or Ctrl+A)
            KeyCode::Char('a') => {
                if key.modifiers.contains(KeyModifiers::META) || key.modifiers.contains(KeyModifiers::CONTROL) {
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
                        let last_line_len = match self.editor.content.get(last_line_idx) {
                            Some(line) => line.len(),
                            None => 0
                        };
                        self.editor.cursor_x = last_line_len;
                    }
                } else {
                    self.insert_char('a');
                }
            }
            // Cut (Cmd+X or Ctrl+X)
            KeyCode::Char('x') => {
                if key.modifiers.contains(KeyModifiers::META) || key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.cut_selection();
                } else {
                    self.insert_char('x');
                }
            }
            // Copy (Cmd+C or Ctrl+C)
            KeyCode::Char('c') => {
                if key.modifiers.contains(KeyModifiers::META) || key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.copy_selection();
                } else {
                    self.insert_char('c');
                }
            }
            // Paste (Cmd+V or Ctrl+V)
            KeyCode::Char('v') => {
                if key.modifiers.contains(KeyModifiers::META) || key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.paste_from_clipboard();
                } else {
                    self.insert_char('v');
                }
            }
            // Terminal-friendly keybindings for wrap guide
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => {
                // Move wrap guide left
                if self.editor.wrap_column > 0 {
                    self.editor.wrap_column -= 1;
                }
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
                // Move wrap guide right
                self.editor.wrap_column += 1;
            }
            // Handle keyboard selection with Shift+Arrow keys
            KeyCode::Left => {
                self.handle_left_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Right => {
                self.handle_right_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Up => {
                self.handle_up_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Down => {
                self.handle_down_key(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            // Regular character input - HJKL keys now work correctly
            KeyCode::Char(c) => {
                self.insert_char(c);
            }
            KeyCode::Enter => {
                let current_line = &self.editor.content[self.editor.cursor_y];
                let remainder = if self.editor.cursor_x < current_line.len() {
                    current_line[self.editor.cursor_x..].to_string()
                } else {
                    String::new()
                };
                self.editor.content[self.editor.cursor_y] = current_line[..self.editor.cursor_x].to_string();
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

    // Common cursor movement handler that manages selection state
    fn handle_cursor_movement(&mut self, 
                              new_y: usize, 
                              new_x: usize, 
                              is_shift_pressed: bool) {
        if is_shift_pressed {
            // Start selection if not already active
            if !self.editor.selection_active {
                self.editor.selection_active = true;
                self.editor.selection_start_x = self.editor.cursor_x;
                self.editor.selection_start_y = self.editor.cursor_y;
            }
            
            // Move cursor to new position
            self.editor.cursor_y = new_y;
            self.editor.cursor_x = new_x;
        } else {
            // Clear selection when moving without shift
            self.editor.selection_active = false;
            
            // Move cursor to new position
            self.editor.cursor_y = new_y;
            self.editor.cursor_x = new_x;
        }
    }

    // Arrow key handlers now use the common movement handler
    fn handle_left_key(&mut self, is_shift_pressed: bool) {
        if self.editor.cursor_x > 0 {
            // Simple move left
            self.handle_cursor_movement(
                self.editor.cursor_y,
                self.editor.cursor_x - 1,
                is_shift_pressed
            );
        } else if self.editor.cursor_y > 0 {
            // Move to end of previous line
            let new_y = self.editor.cursor_y - 1;
            let new_x = match self.editor.content.get(new_y) {
                Some(line) => line.len(),
                None => 0
            };
            self.handle_cursor_movement(new_y, new_x, is_shift_pressed);
        }
    }

    fn handle_right_key(&mut self, is_shift_pressed: bool) {
        let current_line_len = match self.editor.content.get(self.editor.cursor_y) {
            Some(line) => line.len(),
            None => 0
        };
        
        if self.editor.cursor_x < current_line_len {
            // Simple move right
            self.handle_cursor_movement(
                self.editor.cursor_y,
                self.editor.cursor_x + 1,
                is_shift_pressed
            );
        } else if self.editor.cursor_y < self.editor.content.len() - 1 {
            // Move to start of next line
            self.handle_cursor_movement(
                self.editor.cursor_y + 1,
                0,
                is_shift_pressed
            );
        }
    }

    fn handle_up_key(&mut self, is_shift_pressed: bool) {
        if self.editor.cursor_y > 0 {
            let new_y = self.editor.cursor_y - 1;
            let new_x = match self.editor.content.get(new_y) {
                Some(line) => self.editor.cursor_x.min(line.len()),
                None => 0
            };
            self.handle_cursor_movement(new_y, new_x, is_shift_pressed);
        }
    }

    fn handle_down_key(&mut self, is_shift_pressed: bool) {
        if self.editor.cursor_y < self.editor.content.len() - 1 {
            let new_y = self.editor.cursor_y + 1;
            let new_x = match self.editor.content.get(new_y) {
                Some(line) => self.editor.cursor_x.min(line.len()),
                None => 0
            };
            self.handle_cursor_movement(new_y, new_x, is_shift_pressed);
        }
    }

    // Helper method to safely add or update content at a specific position
    fn insert_or_append_at(&mut self, pos: usize, content: String) {
        if pos < self.editor.content.len() {
            self.editor.content.insert(pos, content);
        } else {
            self.editor.content.push(content);
        }
    }

    fn insert_char(&mut self, c: char) {
        if self.editor.cursor_y >= self.editor.content.len() {
            self.editor.content.push(String::new());
        }
        let line = &mut self.editor.content[self.editor.cursor_y];
        if self.editor.cursor_x > line.len() {
            line.push_str(&" ".repeat(self.editor.cursor_x - line.len()));
        }
        line.insert(self.editor.cursor_x, c);
        self.editor.cursor_x += 1;
    }

    fn execute_editor_command(&mut self) {
        match self.editor.command_buffer.as_str() {
            // "v1" => {
            //     self.insert_verse_marker("Verse 1");
            // }
            "split" => {
                if self.editor.cursor_y < self.editor.content.len() {
                    // Don't split the line itself, just insert an empty line at the cursor position
                    self.editor.cursor_y += 1;
                    self.editor.cursor_x = 0;
                    self.editor.content.insert(self.editor.cursor_y, String::new());
                }
            }
            "wrap" => {
                // Apply word wrapping at the current wrap column
                self.editor.content = self.wrap_text(&self.editor.content, self.editor.wrap_column);
            }
            cmd if cmd.starts_with("wrap ") => {
                match cmd[5..].parse::<usize>() {
                    Ok(col) => self.editor.wrap_column = col,
                    Err(_) => {} // Invalid wrap column, do nothing
                }
            }
            _ => {}
        }
    }

    // Extract text wrapping logic to a separate function
    fn wrap_text(&self, content: &[String], wrap_column: usize) -> Vec<String> {
        content
            .iter()
            .flat_map(|line| {
                if line.len() <= wrap_column {
                    // Short lines don't need wrapping
                    return vec![line.clone()];
                }

                let mut result = Vec::new();
                let mut current_line = String::new();
                let words = line.split_whitespace();
                
                for word in words {
                    if current_line.is_empty() {
                        current_line = word.to_string();
                    } else if current_line.len() + word.len() + 1 <= wrap_column {
                        current_line.push(' ');
                        current_line.push_str(word);
                    } else {
                        result.push(current_line);
                        current_line = word.to_string();
                    }
                }
                
                if !current_line.is_empty() {
                    result.push(current_line);
                }
                
                result
            })
            .collect()
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
            tokio::spawn(async move { // Changed from self.runtime.spawn
                let result = client_clone.get_service_items(&plan_id_owned).await;
                // Send the result back to the main thread
                if let Err(_e) = tx_clone.send(AppUpdate::ItemsLoaded(result)).await {
                }
            });
            
            // Don't block here

        } else { 
             // Load dummy items synchronously if no client
            self.load_dummy_items();
        }
    }
    
    fn load_dummy_items(&mut self) {
        self.items = vec![
            Item { id: "dummy_song_1".to_string(), _position: 1, title: "Dummy Song 1".to_string(), _description: None, category: Category::Song, _note: None, song: None, _scripture: None },
            Item { id: "dummy_graphic".to_string(), _position: 2, title: "Dummy Graphic".to_string(), _description: None, category: Category::Graphic, _note: None, song: None, _scripture: None },
            Item { id: "dummy_title".to_string(), _position: 3, title: "Dummy Title".to_string(), _description: None, category: Category::Title, _note: None, song: None, _scripture: None },
            Item { id: "dummy_text".to_string(), _position: 4, title: "Dummy Text".to_string(), _description: None, category: Category::Text, _note: None, song: None, _scripture: None },
            Item { id: "dummy_other".to_string(), _position: 5, title: "Dummy Other".to_string(), _description: None, category: Category::Other, _note: None, song: None, _scripture: None },
        ];
        // Initialize dummy state in HashMaps
        self.item_completion.clear();
        self.item_ignored.clear();
        self.item_matched_file.clear();
        self.item_editor_state.clear();
        for item in &self.items {
            self.item_completion.insert(item.id.clone(), false);
            self.item_ignored.insert(item.id.clone(), false);
            self.item_matched_file.insert(item.id.clone(), None);
            self.item_editor_state.insert(item.id.clone(), None);
        }

        if !self.items.is_empty() {
            self.item_list_state.select(Some(0));
            self.update_matching_files();
        }
    }

    // Helper function to extract item numbers like "#510" from titles
    fn extract_item_number(&self, title: &str) -> Option<String> {
        // Look for patterns like "#123" or "No. 123" 
        if let Some(pos) = title.find('#') {
            // Extract from # to the next space or end of string
            let start = pos + 1;
            let end = title[start..].find(|c: char| !c.is_ascii_digit())
                .map_or(title.len(), |p| p + start);
            if start < end {
                return Some(title[start..end].to_string());
            }
        } 
        // Check if title starts with a number (without #)
        else {
            let trimmed = title.trim();
            if !trimmed.is_empty() && trimmed.chars().next().unwrap().is_ascii_digit() {
                // Get the continuous digits at start
                let end = trimmed.find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(trimmed.len());
                if end > 0 {
                    return Some(trimmed[..end].to_string());
                }
            }
        }
        None
    }

    fn update_matching_files(&mut self) {
        self.matching_files.clear();
        self.file_list_state.select(None);
        
        let selected_item_idx = match self.item_list_state.selected() {
            Some(idx) => idx,
            None => return
        };
        
        // Get the selected item - make a clone to avoid borrow issues
        let selected_item = match self.items.get(selected_item_idx).cloned() {
            Some(item) => item,
            None => return,
        };
        
        // Extract title for searching
        let title = selected_item.title.clone();
        let item_id = selected_item.id.clone();
        
        // Get enhanced search terms - use capacity-optimized Vector for primary terms
        let mut primary_terms = Vec::with_capacity(5);
        primary_terms.push(title.clone());
        
        // Add common liturgical element variations
        let liturgical_mapping = [
            ("Call to Worship", vec!["Call to Worship", "CTW"]),
            ("Prayer of Confession", vec!["Confession", "Prayer of Confession"]),
            ("Greeting", vec!["Greeting", "Welcome"]),
            ("Prayer", vec!["Prayer", "Prayers"]),
            ("Lord's Prayer", vec!["Lord's Prayer", "Our Father"]),
            ("Offertory", vec!["Offertory", "Offering"]),
            ("Doxology", vec!["Doxology", "Gloria Patri", "Praise God"]),
            ("Tithes", vec!["Tithe", "Tithes", "Offering"]),
            ("Offerings", vec!["Offering", "Offerings"]),
            ("Giving", vec!["Giving", "Offering", "Stewardship"]),
            ("Benediction", vec!["Benediction", "Blessing"]),
            ("Scripture", vec!["Scripture", "Bible", "Reading"]),
            ("Anthem", vec!["Anthem", "Choir"]),
        ];
        
        // Check title for liturgical elements and add relevant search terms
        for (key, variations) in &liturgical_mapping {
            if title.contains(key) {
                for term in variations {
                    if !primary_terms.contains(&term.to_string()) {
                        primary_terms.push(term.to_string());
                    }
                }
            }
        }
        
        // For scripture references, add variations with "v" instead of ":"
        if title.contains("Scripture") && title.contains(':') {
            primary_terms.push(title.replace(':', "v"));
        }
        
        // Extract any number references (like "#510") and add as search terms
        if let Some(number) = self.extract_item_number(&title) {
            primary_terms.push(number.clone());
            primary_terms.push(format!("#{}", number));
            primary_terms.push(format!("Hymn {}", number));
            primary_terms.push(format!("[Hymn] {}", number));
            
            // Look for significant words after the hymn number to use as additional terms
            if let Some(pos) = title.find(&number) {
                let after_number = title[pos + number.len()..].trim();
                if !after_number.is_empty() {
                    // Remove articles and common short words
                    let key_words: Vec<&str> = after_number
                        .split_whitespace()
                        .filter(|word| word.len() > 3 && !["with", "from", "your", "thou"].contains(word))
                        .collect();
                    
                    // Add each significant word
                    for word in key_words {
                        if !primary_terms.contains(&word.to_string()) {
                            primary_terms.push(word.to_string());
                        }
                    }
                }
            }
        }
        
        // Handle composite items with "and"
        if title.contains(" and ") {
            // Split by "and" and add each significant part
            let parts: Vec<&str> = title.split(" and ").map(|s| s.trim()).collect();
            
            for part in parts {
                if part.len() > 3 && !primary_terms.contains(&part.to_string()) {
                    primary_terms.push(part.to_string());
                    
                    // Generate variants without common prefixes
                    let clean_part = part.trim_start_matches(|c: char| !c.is_alphanumeric());
                    if clean_part != part && clean_part.len() > 3 {
                        primary_terms.push(clean_part.to_string());
                    }
                }
            }
        }
        
        // For composite terms with slashes like "Prayer/Lord's Prayer"
        if title.contains('/') {
            // Only add individual parts if they're substantial (more than 3 chars)
            for part in title.split('/').map(|s| s.trim()) {
                if part.len() > 3 && !primary_terms.contains(&part.to_string()) {
                    primary_terms.push(part.to_string());
                }
            }
        }
        
        // For specific formats like "Offertory: O Love", add variations
        if title.contains(':') {
            let parts: Vec<&str> = title.split(':').map(|s| s.trim()).collect();
            if parts.len() >= 2 {
                // Add both parts separately - don't filter on length
                if !primary_terms.contains(&parts[0].to_string()) {
                    primary_terms.push(parts[0].to_string());
                }
                if !primary_terms.contains(&parts[1].to_string()) {
                    primary_terms.push(parts[1].to_string());
                }
            }
        }
        
        // For songs, add song title and artist
        if let Some(song) = &selected_item.song {
            // Add song title if different from item title and not already included
            if song.title != title && !song.title.is_empty() && !primary_terms.contains(&song.title) {
                primary_terms.push(song.title.clone());
            }
            
            // Add artist name if available and substantial
            if let Some(author) = &song.author {
                if !author.is_empty() && author.len() > 3 && !primary_terms.contains(author) {
                    primary_terms.push(author.clone());
                }
            }
        }
        
        // Use the file index if available
        if let Some(index) = &self.file_index {
            // Search for primary term first with a larger limit
            let mut all_matches = Vec::new();
            let mut seen_paths = std::collections::HashSet::new();
            
            // Try each primary term
            for term in &primary_terms {
                let matches = index.find_matches(term, 10);
                
                // Add all unique matches to our collection
                for entry in matches {
                    let path_str = entry.full_path.to_string_lossy().to_string();
                    if !seen_paths.contains(&path_str) {
                        seen_paths.insert(path_str);
                        all_matches.push(entry);
                    }
                }
            }
            
            // Self.matching_files 
            self.matching_files = all_matches;
            
            // If we found matches but there's a previous selection for this item,
            // try to restore that selection
            if !self.matching_files.is_empty() && self.matching_files.len() > 1 {
                if let Some(selected_path) = index.get_selection_for_item(&item_id) {
                    // Find the index of the previously selected file
                    if let Some(selected_idx) = self.matching_files.iter().position(|e| 
                        e.full_path.to_string_lossy() == *selected_path
                    ) {
                        // Reorder to put the selected item first
                        if selected_idx > 0 {
                            let selected_entry = self.matching_files.remove(selected_idx);
                            self.matching_files.insert(0, selected_entry);
                        }
                    }
                }
            }
        } else {
            // Fall back to old method if no index
        if self.library_path.is_none() {
            self.update_dummy_matching_files(&title);
            return;
        }
        
        if let Some(lib_path) = &self.library_path {
                // Pass category as well just for compatibility with the old function
                let category = &selected_item.category;
                let items_iter = std::iter::once((&title, category)); 
            let matches = find_matches_for_items(items_iter, lib_path, 10);
            
            if let Some(file_matches) = matches.get(&title) {
                    // Convert strings to dummy FileEntry objects
                    self.matching_files = file_matches.iter()
                        .map(|name| FileEntry {
                            file_name: name.clone(),
                            normalized_name: crate::utils::file_matcher::normalize_name(name),
                            file_name_lower: name.to_lowercase(),
                            normalized_lower: crate::utils::file_matcher::normalize_name(name).to_lowercase(),
                            display_name: name.clone(),
                            _relative_path: String::new(),
                            full_path: PathBuf::new(),
                        })
                        .collect();
                    
                if self.matching_files.is_empty() {
                    self.update_dummy_matching_files(&title);
                }
        } else {
                self.update_dummy_matching_files(&title);
                }
            }
        }
    }
    
    // Helper to provide dummy matching files for testing
    fn update_dummy_matching_files(&mut self, search_term: &str) {
        // For example purposes, populate with mock files based on the selected item name
        let item_name = search_term.to_lowercase();
        
        // Create dummy file entries with owned Strings
        let dummy_entries: Vec<(String, &str)> = if item_name.contains("amazing grace") {
            vec![
                ("Amazing Grace".to_string(), "Songs/Hymns"),
                ("Amazing Grace (My Chains Are Gone)".to_string(), "Songs/Contemporary"),
                ("Amazing Grace (Traditional)".to_string(), "Songs/Traditional"),
            ]
        } else if item_name.contains("how great thou art") {
            vec![
                ("How Great Thou Art".to_string(), "Songs/Hymns"),
                ("How Great Thou Art (Updated)".to_string(), "Songs/Contemporary"),
            ]
        } else if item_name.contains("worship") || item_name.contains("song") {
            vec![
                ("Worship Set 1".to_string(), "Songs/Sets"),
                ("Worship Set 2".to_string(), "Songs/Sets"),
                ("Worship Background".to_string(), "Backgrounds"),
            ]
        } else if item_name.contains("scripture") || item_name.contains("psalm") || item_name.contains("reading") {
            vec![
                ("Scripture Backgrounds".to_string(), "Backgrounds"),
                ("Psalm 23".to_string(), "Scripture"),
                ("Bible Backgrounds".to_string(), "Backgrounds"),
            ]
        } else if item_name.contains("announcements") {
            vec![
                ("Announcements Template".to_string(), "Templates"),
                ("Weekly Announcements".to_string(), "Announcements"),
                ("Announcement Slides".to_string(), "Announcements"),
            ]
        } else if item_name.contains("slide") || item_name.contains("graphic") {
            vec![
                ("Title Slides".to_string(), "Graphics"),
                ("Background Slides".to_string(), "Backgrounds"),
                ("Graphic Templates".to_string(), "Templates"),
            ]
        } else {
            // Generate generic matches for other items
            vec![
                (item_name.clone(), "Presentations"),
                (format!("{} Template", item_name), "Templates"),
                (format!("{} Background", item_name), "Backgrounds"),
            ]
        };
        
        // Convert to FileEntry objects
        self.matching_files = dummy_entries
            .into_iter()
            .map(|(name, path)| FileEntry {
                file_name: name.clone(),
                normalized_name: crate::utils::file_matcher::normalize_name(&name),
                file_name_lower: name.to_lowercase(),
                normalized_lower: crate::utils::file_matcher::normalize_name(&name).to_lowercase(),
                display_name: name,
                _relative_path: path.to_string(),
                full_path: PathBuf::new(),
            })
            .collect();
    }

    fn try_generate_playlist(&mut self) {
        // Count how many items are neither completed nor ignored
        let uncompleted_count = self.items.iter()
            .filter(|item| {
                let id = &item.id;
                let is_completed = *self.item_completion.get(id).unwrap_or(&false);
                let is_ignored = *self.item_ignored.get(id).unwrap_or(&false);
                !is_completed && !is_ignored
            })
            .count();
            
        if uncompleted_count > 0 {
            // TODO: Show confirmation modal
            self.error_message = Some(format!("Warning: {} items are not matched or marked to ignore!", uncompleted_count));
        } else {
            // TODO: Generate playlist using item data and matched files from maps, skipping ignored items
            self.error_message = Some("Playlist generation not implemented yet.".to_string());
        }
    }

    /// Write text to system clipboard (silently ignores errors)
    fn clipboard_write(&self, text: &str) {
        let _ = Clipboard::new().and_then(|mut cb| cb.set_text(text.to_owned()));
    }

    /// Read text from system clipboard
    fn clipboard_read(&self) -> Option<String> {
        Clipboard::new().ok()?.get_text().ok()
    }

    fn get_selected_text(&self) -> String {
        if !self.editor.selection_active {
            // No selection: return current line (VSCode behavior)
            return self.editor.content.get(self.editor.cursor_y)
                .map(|line| format!("{}\n", line))
                .unwrap_or_default();
        }

        let (start_y, start_x, end_y, end_x) = self.get_selection_bounds();
        
        if start_y == end_y {
            // Single line selection
            return self.editor.content.get(start_y)
                .map(|line| {
                    let end = end_x.min(line.len());
                    if start_x <= end { line[start_x..end].to_string() } else { String::new() }
                })
                .unwrap_or_default();
        }
        
        // Selection spans multiple lines
        let mut result = String::new();
        
        // First line
        if let Some(line) = self.editor.content.get(start_y) {
            let start_idx = start_x.min(line.len());
            if start_idx < line.len() {
                result.push_str(&line[start_idx..]);
            }
            result.push('\n');
        }
        
        // Middle lines - use iterator approach
        result.extend(
            self.editor.content.iter()
                .skip(start_y + 1)
                .take(end_y - start_y - 1)
                .flat_map(|line| [line.as_str(), "\n"].into_iter())
        );
        
        // Last line
        if let Some(line) = self.editor.content.get(end_y) {
            let end_idx = end_x.min(line.len());
            result.push_str(&line[..end_idx]);
        }
        
        result
    }
    
    // Ensure there's always an empty line at the end of content
    fn ensure_empty_line_at_end(&mut self) {
        if self.editor.content.is_empty() {
            self.editor.content.push(String::new());
            return;
        }
        
        let last_idx = self.editor.content.len() - 1;
        if !self.editor.content[last_idx].is_empty() {
            self.editor.content.push(String::new());
        } else if self.editor.content.len() == 1 && self.editor.content[0].is_empty() {
            // Already has exactly one empty line, nothing to do
            return;
        } else if last_idx > 0 && self.editor.content[last_idx-1].is_empty() && self.editor.content[last_idx].is_empty() {
            // Already has multiple empty lines, reduce to just one
            self.editor.content.truncate(last_idx + 1);
        }
    }

    fn delete_selected_text(&mut self) {
        if !self.editor.selection_active {
            return;
        }
        
        // Determine the actual start and end points
        let (start_y, start_x, end_y, end_x) = self.get_selection_bounds();
        
        if start_y == end_y {
            // Selection is on a single line
            match self.editor.content.get_mut(start_y) {
                Some(line) => {
                    let end_idx = end_x.min(line.len());
                    if start_x < end_idx {
                        let after = line[end_idx..].to_string();
                        line.truncate(start_x);
                        line.push_str(&after);
                    }
                }
                None => {}
            }
        } else {
            // Selection spans multiple lines
            let mut new_content = Vec::new();
            
            // Add lines before selection
            new_content.extend(self.editor.content[0..start_y].iter().cloned());
            
            // Add first line (up to selection start) + last line (from selection end)
            let first_part = match self.editor.content.get(start_y) {
                Some(line) => line[..start_x.min(line.len())].to_string(),
                None => String::new()
            };
            
            let last_part = match self.editor.content.get(end_y) {
                Some(line) => {
                    let end_idx = end_x.min(line.len());
                    line[end_idx..].to_string()
                },
                None => String::new()
            };
            
            // Combine the parts and add to new content
            new_content.push(first_part + &last_part);
            
            // Add lines after selection
            new_content.extend(self.editor.content[end_y + 1..].iter().cloned());
            
            self.editor.content = new_content;
        }
        
        // Reset cursor to start of selection
        self.editor.cursor_y = start_y;
        self.editor.cursor_x = start_x;
        self.editor.selection_active = false;
    }
    
    fn get_selection_bounds(&self) -> (usize, usize, usize, usize) {
        if !self.editor.selection_active {
            // If no selection, return cursor position for both start and end
            return (
                self.editor.cursor_y, 
                self.editor.cursor_x, 
                self.editor.cursor_y, 
                self.editor.cursor_x
            );
        }
        
        // Determine start and end points based on selection direction
        let (start_y, start_x, end_y, end_x) = if (self.editor.selection_start_y < self.editor.cursor_y) || 
           (self.editor.selection_start_y == self.editor.cursor_y && self.editor.selection_start_x < self.editor.cursor_x) {
            // Normal selection (top to bottom)
            (
                self.editor.selection_start_y, 
                self.editor.selection_start_x, 
                self.editor.cursor_y, 
                self.editor.cursor_x
            )
        } else {
            // Reverse selection (bottom to top)
            (
                self.editor.cursor_y, 
                self.editor.cursor_x, 
                self.editor.selection_start_y, 
                self.editor.selection_start_x
            )
        };
        
        (start_y, start_x, end_y, end_x)
    }

    // Function to find the start and end lines of the paragraph containing the cursor
    pub fn get_current_paragraph_bounds(&self) -> Option<(usize, usize)> {
        let y = self.editor.cursor_y;
        
        // Ensure cursor is within content bounds
        if y >= self.editor.content.len() {
            return None;
        }

        // Find the start of the paragraph (first non-empty line after an empty line or start of doc)
        let start_y = (0..=y)
            .rev()
            .find(|&i| i == 0 || self.editor.content.get(i - 1).map_or(false, |line| line.is_empty()))
            .unwrap_or(y); // Should always find at least y
            
        // If the line at start_y is itself empty, it's not really a paragraph start
        if self.editor.content.get(start_y).map_or(true, |line| line.is_empty()) {
            return None;
        }

        // Find the end of the paragraph (last non-empty line before an empty line or end of doc)
        let end_y = (y..self.editor.content.len())
            .find(|&i| self.editor.content.get(i).map_or(true, |line| line.is_empty()))
            .map_or(self.editor.content.len() - 1, |i| i.saturating_sub(1));
            
        // Ensure start_y is actually before or at end_y (handles edge cases)
        if start_y <= end_y {
            Some((start_y, end_y))
        } else {
            None // This can happen if the cursor is on an isolated empty line
        }
    }
    
    // Helper function to determine if cursor is in a stanza
    fn is_cursor_in_stanza(&self) -> bool {
        // Look for non-empty lines above and below the cursor
        let cursor_y = self.editor.cursor_y;
        
        // Check if line at cursor is non-empty
        let current_line_empty = self.editor.content
            .get(cursor_y)
            .map_or(true, |line| line.is_empty());
        
        if !current_line_empty {
            return true;
        }
        
        // If cursor is on an empty line, check adjacent lines
        
        // Check if any non-empty line exists above
        let has_text_above = self.editor.content
            .iter()
            .take(cursor_y)
            .rev()
            .take_while(|line| line.is_empty())
            .count() < cursor_y;
        
        // Check if any non-empty line exists below
        let has_text_below = self.editor.content
            .iter()
            .skip(cursor_y + 1)
            .take_while(|line| line.is_empty())
            .count() < self.editor.content.len() - cursor_y - 1;
        
        // Cursor is in a stanza if there are non-empty lines both above and below
        has_text_above && has_text_below
    }
    
    // Find the start of the current stanza
    fn find_stanza_start(&self, y: usize) -> usize {
        // Find the first empty line above the current position
        self.editor.content
            .iter()
            .take(y)
            .enumerate()
            .rev()
            .find_map(|(i, line)| {
                if line.is_empty() {
                    Some(i + 1) // Start of stanza is one line after the empty line
                } else {
                    None
                }
            })
            .unwrap_or(0) // If no empty line found, start is at beginning of document
    }
    
    /// Insert a verse marker (e.g., "[Verse 1]") with appropriate blank line handling
    fn insert_verse_marker(&mut self, marker_text: &str) {
        let marker_line = format!("[{}]", marker_text);
        let cursor_y = self.editor.cursor_y;
        let content_len = self.editor.content.len();

        // Check if cursor is touching a stanza (inside or directly below)
        let is_touching_stanza = self.is_cursor_in_stanza() || 
            (cursor_y > 0 && 
             cursor_y < content_len && 
             self.editor.content.get(cursor_y - 1).map_or(false, |line| !line.is_empty()) &&
             self.editor.content.get(cursor_y).map_or(false, |line| line.is_empty()));

        if is_touching_stanza {
            // Insert within or before a stanza
            let original_cursor_y = cursor_y; 
            let original_cursor_x = self.editor.cursor_x;
            
            let effective_y = if self.is_cursor_in_stanza() { cursor_y } else { cursor_y - 1 }; 
            let stanza_start = self.find_stanza_start(effective_y);
            let mut insert_pos = stanza_start;
            let mut lines_inserted_above = 0;

            // Ensure blank line above if needed
            if stanza_start > 0 && self.editor.content.get(stanza_start - 1).map_or(false, |line| !line.is_empty()) {
                self.editor.content.insert(insert_pos, String::new());
                insert_pos += 1;
                lines_inserted_above += 1;
            }
            
            // Insert marker
            self.editor.content.insert(insert_pos, marker_line);
            lines_inserted_above += 1;
            
            // Restore cursor position, adjusted for inserted lines
            self.editor.cursor_y = original_cursor_y + lines_inserted_above;
            self.editor.cursor_x = original_cursor_x;
        } else {
            // Insert standalone marker
            let mut marker_idx = cursor_y;

            // Place the marker line
            if cursor_y < content_len && self.editor.content[cursor_y].is_empty() {
                self.editor.content[cursor_y] = marker_line;
            } else {
                self.editor.content.insert(cursor_y, marker_line);
            }

            // Ensure blank line BEFORE marker (unless at top)
            if marker_idx > 0 && self.editor.content.get(marker_idx - 1).map_or(false, |line| !line.is_empty()) {
                self.editor.content.insert(marker_idx, String::new());
                marker_idx += 1;
            }

            // Ensure blank line AFTER marker
            let after_idx = marker_idx + 1;
            if after_idx >= self.editor.content.len() {
                self.editor.content.push(String::new());
            } else if self.editor.content.get(after_idx).map_or(false, |line| !line.is_empty()) {
                self.editor.content.insert(after_idx, String::new());
            }

            // Position cursor 2 lines after marker
            let target_y = marker_idx + 2;
            while target_y >= self.editor.content.len() {
                self.editor.content.push(String::new());
            }
            self.editor.cursor_y = target_y;
            self.editor.cursor_x = 0;
        }
        
        // Final clamp and ensure empty line at end
        self.editor.cursor_y = self.editor.cursor_y.min(self.editor.content.len().saturating_sub(1));
        self.ensure_empty_line_at_end();
    }
    
    // Copy selection or current line to clipboard
    fn copy_selection(&mut self) {
        if !self.editor.selection_active {
            // If no selection is active, copy the current line
            match self.editor.content.get(self.editor.cursor_y) {
                Some(line) => {
                    // Add a newline at the end to match typical editor behavior
                    self.clipboard_write(&format!("{}\n", line));
                }
                None => {}
            }
            return;
        }

        // Copy the selected text
        let selected_text = self.get_selected_text();
        self.clipboard_write(&selected_text);
    }
    
    // Cut selection or current line to clipboard
    fn cut_selection(&mut self) {
        if self.editor.selection_active {
            let selected_text = self.get_selected_text();
            if !selected_text.is_empty() {
                self.clipboard_write(&selected_text);
                self.delete_selected_text();
            }
        } else if !self.editor.content.is_empty() && self.editor.cursor_y < self.editor.content.len() {
            // Fall back to cutting current line if no selection
            let line = self.editor.content.remove(self.editor.cursor_y);
            self.clipboard_write(&(line + "\n"));
            
            // If we removed the last line, add an empty one
            if self.editor.content.is_empty() {
                self.editor.content.push(String::new());
            }
            
            // Adjust cursor position
            if self.editor.cursor_y >= self.editor.content.len() {
                self.editor.cursor_y = self.editor.content.len() - 1;
            }
            self.editor.cursor_x = 0;
        }
        self.editor.selection_active = false;
    }
    
    // Paste from clipboard at current cursor position
    fn paste_from_clipboard(&mut self) {
        // Delete selected text before pasting if selection is active
        if self.editor.selection_active {
            self.delete_selected_text();
            self.editor.selection_active = false;
        }
        
        // Paste from clipboard
        match self.clipboard_read() {
            Some(content) => {
                let normalized_content = content.replace("\r\n", "\n"); // Normalize line endings
                
                // Split content by lines, keeping trailing newlines
                let lines: Vec<&str> = normalized_content.split('\n').collect();
                let line_count = lines.len();
                
                if line_count == 1 || (line_count == 2 && lines[1].is_empty()) {
                    // Single line paste - insert at cursor
                    match self.editor.content.get_mut(self.editor.cursor_y) {
                        Some(line) => {
                            if self.editor.cursor_x > line.len() {
                                line.push_str(&" ".repeat(self.editor.cursor_x - line.len()));
                            }
                            line.insert_str(self.editor.cursor_x, lines[0]);
                            self.editor.cursor_x += lines[0].len();
                        }
                        None => {}
                    }
                } else {
                    // Multiline paste
                    
                    // First, handle the current line
                    let current_line = match self.editor.content.get(self.editor.cursor_y) {
                        Some(line) => {
                            let x = self.editor.cursor_x.min(line.len());
                            let before = line[..x].to_string();
                            let after = line[x..].to_string();
                            (before, after)
                        },
                        None => (String::new(), String::new())
                    };
                    
                    // Update current line with first part of pasted content
                    if self.editor.cursor_y < self.editor.content.len() {
                        self.editor.content[self.editor.cursor_y] = current_line.0 + lines[0];
                    }
                    
                    // Insert middle lines
                    let mut insert_pos = self.editor.cursor_y + 1;
                    
                    // Add all middle lines (skipping first and last)
                    for &line in &lines[1..line_count-1] {
                        self.insert_or_append_at(insert_pos, line.to_string());
                        insert_pos += 1;
                    }
                    
                    // Insert last line + remaining content
                    if line_count > 1 {
                        let last_line = lines[line_count - 1];
                        let new_line = last_line.to_string() + &current_line.1;
                        
                        self.insert_or_append_at(insert_pos, new_line);
                        
                        // Update cursor position to end of pasted content
                        self.editor.cursor_y = insert_pos;
                        self.editor.cursor_x = last_line.len();
                    }
                }
            }
            None => {}
        }
    }

    // Renamed function to better reflect loading types and plans
    fn initialize_data(&mut self) {
        // Set loading state immediately
        self.is_loading = true;
        
        if let Some(client) = &self.pco_client {
            let client_clone = client.clone();
            let config_clone = self.config.clone(); 
            let tx_clone = self.async_task_tx.clone();

            // Spawn the async task using tokio::spawn
            tokio::spawn(async move {
                let result = client_clone.get_upcoming_services(config_clone.days_ahead).await;
                if let Err(_e) = tx_clone.send(AppUpdate::DataLoaded(result)).await {
                }
            });
        } else {
           // Load dummy data synchronously
           self.initialize_selection_state();
           self.is_loading = false; 
        }
    }

    // Helper function to set initial selection state after data is loaded
    fn initialize_selection_state(&mut self) {
        // eprintln!("[initialize_selection_state] Setting initial selection..."); // REMOVED
        if !self.services.is_empty() {
            self.service_list_state.select(Some(0));
            self.active_service_id = self.services.get(0).map(|s| s.id.clone()); 
            // eprintln!("  Selected service index: 0, Active ID: {:?}", self.active_service_id); // REMOVED
        } else {
            self.service_list_state.select(None);
            self.active_service_id = None;
            // eprintln!("  No services, selection cleared."); // REMOVED
        }
        self.plan_list_state.select(None);
    }

    // New method to handle updates from async tasks
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
                            },
                            Err(_e) => { 
                                self.error_message = Some(format!("Failed to load services: {}", _e));
                            }
                        }
                    },
                    AppUpdate::ItemsLoaded(result) => {
                        self.is_loading = false; // Stop loading indicator
                        match result {
                            Ok(items) => {
                                self.items = items;
                                
                                // Initialize state for items
                                self.item_completion.clear();
                                self.item_ignored.clear();
                                self.item_matched_file.clear();
                                self.item_editor_state.clear();
                                
                                for item in &self.items {
                                    self.item_completion.insert(item.id.clone(), false);
                                    self.item_ignored.insert(item.id.clone(), false);
                                    self.item_matched_file.insert(item.id.clone(), None);
                                    self.item_editor_state.insert(item.id.clone(), None);
                                }
                                
                                if !self.items.is_empty() {
                                    self.item_list_state.select(Some(0));
                                    self.update_matching_files();
                                }
                            },
                            Err(_e) => {
                                self.error_message = Some(format!("Failed to load service items: {}", _e));
                            }
                        }
                    },
                }
            },
            Err(mpsc::error::TryRecvError::Empty) => { },
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // Channel disconnected - could log this if needed
            },
        }
    }

    // Add a method to retry loading data
    pub fn retry_data_loading(&mut self) {
        // Clear error message if present
        self.error_message = None;
        
        match self.mode {
            AppMode::ServiceList => {
                // Retry loading services and plans
                self.initialize_data();
            },
            AppMode::ItemList => {
                // If we have a selected plan, retry loading its items
                let plan_id = self.get_selected_plan_id();
                if let Some(id) = plan_id {
                    self.load_items_for_plan(&id);
                } else {
                    self.error_message = Some("No plan selected to reload".to_string());
                }
            },
            _ => {} // Other modes don't have data to reload
        }
    }
    
    // Helper method to get the currently selected plan ID
    fn get_selected_plan_id(&self) -> Option<String> {
        if let Some(selected_idx_filtered) = self.plan_list_state.selected() {
            if let Some(service_id) = &self.active_service_id {
                let filtered_plans: Vec<_> = self.plans.iter()
                    .filter(|p| &p.service_id == service_id)
                    .collect();
                
                if let Some(plan) = filtered_plans.get(selected_idx_filtered) {
                    return Some(plan.id.clone());
                }
            }
        }
        None
    }

    // Method to handle selecting a file for the current item
    fn select_file_for_item(&mut self) {
        // Get the selected file index
        let selected_file_idx = match self.file_list_state.selected() {
            Some(idx) => idx,
            None => return,
        };
        
        // Get the selected item
        let selected_item_idx = match self.item_list_state.selected() {
            Some(idx) => idx,
            None => return,
        };
        
        let selected_item = match self.items.get(selected_item_idx) {
            Some(item) => item,
            None => return,
        };
        
        // Get the selected file entry
        let selected_file = match self.matching_files.get(selected_file_idx) {
            Some(file) => file,
            None => return,
        };
        
        // Record the selection in our item_matched_file hashmap
        let item_id = selected_item.id.clone();
        let file_path = selected_file.full_path.to_string_lossy().to_string();
        self.item_matched_file.insert(item_id.clone(), Some(file_path.clone()));
        
        // Mark the item as completed
        self.item_completion.insert(item_id.clone(), true);
        
        // Record the selection in the file index for better future ranking
        if let Some(index) = &mut self.file_index {
            index.record_selection(&item_id, &selected_file.full_path);
        }
        
        // Move to the next item if possible
        if let Some(next_idx) = self.find_next_uncompleted_item(selected_item_idx) {
            self.item_list_state.select(Some(next_idx));
            self.update_matching_files();
        }
    }

    // Helper to find next uncompleted item index
    fn find_next_uncompleted_item(&self, current_idx: usize) -> Option<usize> {
        ((current_idx + 1)..self.items.len())
            .find(|&i| {
                if let Some(item) = self.items.get(i) {
                    let is_completed = *self.item_completion.get(&item.id).unwrap_or(&false);
                    let is_ignored = *self.item_ignored.get(&item.id).unwrap_or(&false);
                    
                    // Skip both completed and ignored items
                    !is_completed && !is_ignored
                } else {
                    false
                }
            })
    }
} 