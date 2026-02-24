//! Main TUI application state machine.
//!
//! Manages view navigation, state transitions, status messages, and input
//! routing. The `App` struct is the top-level owner of all UI state — it
//! does not perform I/O or communicate with the daemon; it only tracks
//! what the user is looking at and what they have typed.

use crate::completion::Completer;
use crate::input::InputLine;


// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// The current view the user is looking at.
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    /// Initial startup / loading screen.
    Startup,
    /// Main dashboard showing agents, tasks, and projects.
    Dashboard,
    /// Detailed view of a single agent.
    AgentDetail { name: String },
    /// Detailed view of a single task.
    TaskDetail { id: String },
    /// Configuration settings view.
    ConfigView,
    /// Scrollable log view.
    LogView,
    /// Help view, optionally for a specific topic.
    HelpView { topic: Option<String> },
    /// Command entry mode (input prompt is active).
    CommandEntry,
    /// Confirmation dialog before a destructive action.
    Confirm {
        prompt: String,
        action: PendingAction,
    },
}


impl AppState {
    /// Return a short label for this state, suitable for display in headers.
    pub fn label(&self) -> &str {
        match self {
            AppState::Startup => "startup",
            AppState::Dashboard => "dashboard",
            AppState::AgentDetail { .. } => "agent",
            AppState::TaskDetail { .. } => "task",
            AppState::ConfigView => "config",
            AppState::LogView => "log",
            AppState::HelpView { .. } => "help",
            AppState::CommandEntry => "command",
            AppState::Confirm { .. } => "confirm",
        }
    }
}


// ---------------------------------------------------------------------------
// PendingAction
// ---------------------------------------------------------------------------

/// An action that requires user confirmation before executing.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    /// Kill an agent by name.
    KillAgent { name: String },
    /// Restart an agent by name.
    RestartAgent { name: String },
    /// Cancel a task by ID.
    CancelTask { id: String },
    /// Remove a project by name.
    RemoveProject { name: String },
    /// A custom action described by free-text and a JSON command payload.
    Custom {
        description: String,
        command_json: String,
    },
}


impl PendingAction {
    /// Return a short description of the pending action.
    pub fn description(&self) -> String {
        match self {
            PendingAction::KillAgent { name } => format!("Kill agent '{}'", name),
            PendingAction::RestartAgent { name } => format!("Restart agent '{}'", name),
            PendingAction::CancelTask { id } => format!("Cancel task '{}'", id),
            PendingAction::RemoveProject { name } => format!("Remove project '{}'", name),
            PendingAction::Custom { description, .. } => description.clone(),
        }
    }
}


// ---------------------------------------------------------------------------
// AppAction
// ---------------------------------------------------------------------------

/// An action produced by the application in response to user input.
#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    /// Send a command string to the daemon.
    SendCommand(String),
    /// Navigate to a new view.
    Navigate(AppState),
    /// Quit the application.
    Quit,
    /// Refresh the current view.
    Refresh,
    /// Scroll the current view up.
    ScrollUp,
    /// Scroll the current view down.
    ScrollDown,
    /// Select the next item in a list.
    SelectNext,
    /// Select the previous item in a list.
    SelectPrev,
    /// Confirm a pending action.
    Confirm,
    /// Cancel the current action or dialog.
    Cancel,
}


// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Top-level application state for the MuxUX TUI.
///
/// Owns the current view state, navigation stack, input line, completer,
/// and display-related bookkeeping. Does not perform any I/O.
pub struct App {
    /// Current view state.
    pub state: AppState,
    /// Stack of previous states for back-navigation.
    previous_states: Vec<AppState>,
    /// Transient status message displayed at the bottom of the screen.
    status_message: Option<(String, u64)>,
    /// Time-to-live for status messages in milliseconds.
    status_ttl_ms: u64,
    /// Command input line.
    pub input: InputLine,
    /// Tab completer for commands.
    pub completer: Completer,
    /// Index of the currently selected item in a list view.
    pub selected_index: usize,
    /// Scroll offset for views that support scrolling.
    pub scroll_offset: usize,
    /// Timestamp (ms) of the last data refresh.
    last_refresh_ms: u64,
    /// How often (ms) to auto-refresh data.
    pub refresh_interval_ms: u64,
}


impl App {
    /// Create a new App in the Startup state.
    pub fn new() -> Self {
        App {
            state: AppState::Startup,
            previous_states: Vec::new(),
            status_message: None,
            status_ttl_ms: 5000,
            input: InputLine::new(),
            completer: Completer::with_default_commands(),
            selected_index: 0,
            scroll_offset: 0,
            last_refresh_ms: 0,
            refresh_interval_ms: 2000,
        }
    }

    // -------------------------------------------------------------------
    // State transitions
    // -------------------------------------------------------------------

    /// Transition to a new state, pushing the current state onto the stack.
    pub fn transition(&mut self, new_state: AppState) {
        let old = std::mem::replace(&mut self.state, new_state);
        self.previous_states.push(old);
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Navigate back to the previous state. Returns the popped state, or
    /// `None` if the stack is empty.
    pub fn back(&mut self) -> Option<AppState> {
        if let Some(prev) = self.previous_states.pop() {
            let current = std::mem::replace(&mut self.state, prev);
            self.selected_index = 0;
            self.scroll_offset = 0;
            Some(current)
        } else {
            None
        }
    }

    /// Push a state onto the navigation stack and switch to it.
    /// Alias for `transition`.
    pub fn push_state(&mut self, state: AppState) {
        self.transition(state);
    }

    /// Pop the navigation stack and return to the previous view.
    /// Alias for `back`.
    pub fn pop_state(&mut self) -> Option<AppState> {
        self.back()
    }

    /// Navigate directly to a state, clearing the stack first.
    pub fn navigate_to(&mut self, state: AppState) {
        self.previous_states.clear();
        self.state = state;
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Return the label of the current view.
    pub fn current_view(&self) -> &str {
        self.state.label()
    }

    /// Return the depth of the navigation stack.
    pub fn stack_depth(&self) -> usize {
        self.previous_states.len()
    }

    // -------------------------------------------------------------------
    // Status messages
    // -------------------------------------------------------------------

    /// Set a transient status message with the given timestamp.
    pub fn set_status(&mut self, msg: &str, now_ms: u64) {
        self.status_message = Some((msg.to_string(), now_ms));
    }

    /// Clear the status message if it has expired relative to `now_ms`.
    pub fn clear_expired_status(&mut self, now_ms: u64) {
        if let Some((_, created)) = &self.status_message {
            if now_ms.saturating_sub(*created) >= self.status_ttl_ms {
                self.status_message = None;
            }
        }
    }

    /// Return the current status message, if any.
    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_ref().map(|(msg, _)| msg.as_str())
    }

    /// Clear the status message immediately.
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    // -------------------------------------------------------------------
    // Refresh timing
    // -------------------------------------------------------------------

    /// Return whether the view needs a data refresh based on `now_ms`.
    pub fn needs_refresh(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_refresh_ms) >= self.refresh_interval_ms
    }

    /// Record that a refresh just happened at `now_ms`.
    pub fn mark_refreshed(&mut self, now_ms: u64) {
        self.last_refresh_ms = now_ms;
    }

    // -------------------------------------------------------------------
    // Input processing
    // -------------------------------------------------------------------

    /// Process a key event and return an optional action.
    ///
    /// Routing depends on the current state:
    /// - In `Confirm` state, only 'y', 'n', Enter, and Escape are handled.
    /// - In `CommandEntry` state, keys go to the input line.
    /// - In other states, keys are routed to view-level shortcuts.
    pub fn handle_key(&mut self, key: Key) -> Option<AppAction> {
        match &self.state {
            AppState::Confirm { .. } => self.handle_confirm_key(key),
            AppState::CommandEntry => self.handle_command_key(key),
            _ => self.handle_view_key(key),
        }
    }

    fn handle_confirm_key(&mut self, key: Key) -> Option<AppAction> {
        match key {
            Key::Char('y') | Key::Char('Y') | Key::Enter => Some(AppAction::Confirm),
            Key::Char('n') | Key::Char('N') | Key::Escape => Some(AppAction::Cancel),
            _ => None,
        }
    }

    fn handle_command_key(&mut self, key: Key) -> Option<AppAction> {
        match key {
            Key::Escape => {
                self.input.clear();
                Some(AppAction::Cancel)
            }
            Key::Enter => {
                let text = self.input.submit();
                if text.is_empty() {
                    Some(AppAction::Cancel)
                } else {
                    Some(AppAction::SendCommand(text))
                }
            }
            Key::Tab => {
                self.handle_tab();
                None
            }
            Key::Backspace => {
                self.input.delete_back();
                None
            }
            Key::Delete => {
                self.input.delete_forward();
                None
            }
            Key::Left => {
                self.input.move_left();
                None
            }
            Key::Right => {
                self.input.move_right();
                None
            }
            Key::Home => {
                self.input.move_home();
                None
            }
            Key::End => {
                self.input.move_end();
                None
            }
            Key::Up => {
                self.input.history_up();
                None
            }
            Key::Down => {
                self.input.history_down();
                None
            }
            Key::Char(ch) => {
                self.input.insert(ch);
                None
            }
            Key::Ctrl('w') => {
                self.input.delete_word_back();
                None
            }
            Key::Ctrl('a') => {
                self.input.move_home();
                None
            }
            Key::Ctrl('e') => {
                self.input.move_end();
                None
            }
            Key::Ctrl('u') => {
                self.input.clear();
                None
            }
            _ => None,
        }
    }

    fn handle_view_key(&mut self, key: Key) -> Option<AppAction> {
        match key {
            Key::Char('q') => Some(AppAction::Quit),
            Key::Char('?') => Some(AppAction::Navigate(AppState::HelpView { topic: None })),
            Key::Char('/') => {
                self.transition(AppState::CommandEntry);
                None
            }
            Key::Char(':') => {
                self.transition(AppState::CommandEntry);
                None
            }
            Key::Char('r') => Some(AppAction::Refresh),
            Key::Char('j') | Key::Down => Some(AppAction::SelectNext),
            Key::Char('k') | Key::Up => Some(AppAction::SelectPrev),
            Key::Char('G') | Key::End => Some(AppAction::ScrollDown),
            Key::Char('g') | Key::Home => Some(AppAction::ScrollUp),
            Key::Enter => {
                // Enter on a selected item could navigate to detail
                None
            }
            Key::Escape => {
                if self.back().is_some() {
                    None
                } else {
                    Some(AppAction::Cancel)
                }
            }
            Key::PageDown => Some(AppAction::ScrollDown),
            Key::PageUp => Some(AppAction::ScrollUp),
            _ => None,
        }
    }

    /// Perform tab completion on the current input.
    pub fn handle_tab(&mut self) {
        let text = self.input.text();
        let cursor = self.input.cursor_pos();
        let result = self.completer.complete(&text, cursor);
        if !result.common_prefix.is_empty() && result.common_prefix.len() > text.len() {
            // Replace input with the common prefix
            self.input.clear();
            for ch in result.common_prefix.chars() {
                self.input.insert(ch);
            }
            if result.complete {
                self.input.insert(' ');
            }
        }
    }

    /// Submit the current input and return the text, or `None` if empty.
    pub fn handle_enter(&mut self) -> Option<String> {
        let text = self.input.submit();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    // -------------------------------------------------------------------
    // Selection helpers
    // -------------------------------------------------------------------

    /// Move the selection index up, clamping to 0.
    pub fn select_prev(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    /// Move the selection index down, clamping to `max_index`.
    pub fn select_next(&mut self, max_index: usize) {
        if self.selected_index < max_index {
            self.selected_index += 1;
        }
    }

    /// Scroll up by one line.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Scroll down by one line, clamping to `max_offset`.
    pub fn scroll_down(&mut self, max_offset: usize) {
        if self.scroll_offset < max_offset {
            self.scroll_offset += 1;
        }
    }

    /// Scroll up by a page.
    pub fn page_up(&mut self, page_size: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
    }

    /// Scroll down by a page, clamping to `max_offset`.
    pub fn page_down(&mut self, page_size: usize, max_offset: usize) {
        self.scroll_offset = (self.scroll_offset + page_size).min(max_offset);
    }
}


impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}


// ---------------------------------------------------------------------------
// Key
// ---------------------------------------------------------------------------

/// A simplified key event for the TUI.
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Ctrl(char),
    Alt(char),
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Construction ---

    #[test]
    fn new_starts_in_startup() {
        let app = App::new();
        assert_eq!(app.state, AppState::Startup);
        assert_eq!(app.current_view(), "startup");
    }

    #[test]
    fn default_is_new() {
        let app = App::default();
        assert_eq!(app.state, AppState::Startup);
    }

    #[test]
    fn new_has_empty_stack() {
        let app = App::new();
        assert_eq!(app.stack_depth(), 0);
    }

    #[test]
    fn new_has_no_status() {
        let app = App::new();
        assert!(app.status_message().is_none());
    }

    #[test]
    fn new_selected_index_zero() {
        let app = App::new();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn new_scroll_offset_zero() {
        let app = App::new();
        assert_eq!(app.scroll_offset, 0);
    }

    // --- State transitions ---

    #[test]
    fn transition_pushes_old_state() {
        let mut app = App::new();
        app.transition(AppState::Dashboard);
        assert_eq!(app.state, AppState::Dashboard);
        assert_eq!(app.stack_depth(), 1);
    }

    #[test]
    fn transition_resets_selection() {
        let mut app = App::new();
        app.selected_index = 5;
        app.scroll_offset = 10;
        app.transition(AppState::Dashboard);
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn back_pops_state() {
        let mut app = App::new();
        app.transition(AppState::Dashboard);
        app.transition(AppState::AgentDetail { name: "w1".into() });
        assert_eq!(app.stack_depth(), 2);

        let popped = app.back();
        assert!(popped.is_some());
        assert_eq!(app.state, AppState::Dashboard);
        assert_eq!(app.stack_depth(), 1);
    }

    #[test]
    fn back_empty_stack_returns_none() {
        let mut app = App::new();
        let popped = app.back();
        assert!(popped.is_none());
        assert_eq!(app.state, AppState::Startup);
    }

    #[test]
    fn back_resets_selection() {
        let mut app = App::new();
        app.transition(AppState::Dashboard);
        app.selected_index = 3;
        app.back();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn push_state_is_transition() {
        let mut app = App::new();
        app.push_state(AppState::Dashboard);
        assert_eq!(app.state, AppState::Dashboard);
        assert_eq!(app.stack_depth(), 1);
    }

    #[test]
    fn pop_state_is_back() {
        let mut app = App::new();
        app.push_state(AppState::Dashboard);
        let popped = app.pop_state();
        assert!(popped.is_some());
        assert_eq!(app.state, AppState::Startup);
    }

    #[test]
    fn navigate_to_clears_stack() {
        let mut app = App::new();
        app.transition(AppState::Dashboard);
        app.transition(AppState::ConfigView);
        assert_eq!(app.stack_depth(), 2);

        app.navigate_to(AppState::LogView);
        assert_eq!(app.state, AppState::LogView);
        assert_eq!(app.stack_depth(), 0);
    }

    #[test]
    fn navigate_to_resets_selection() {
        let mut app = App::new();
        app.selected_index = 5;
        app.scroll_offset = 10;
        app.navigate_to(AppState::Dashboard);
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn multi_level_navigation() {
        let mut app = App::new();
        app.transition(AppState::Dashboard);
        app.transition(AppState::AgentDetail { name: "w1".into() });
        app.transition(AppState::HelpView { topic: Some("agent".into()) });
        assert_eq!(app.stack_depth(), 3);

        app.back();
        assert_eq!(app.state, AppState::AgentDetail { name: "w1".into() });
        app.back();
        assert_eq!(app.state, AppState::Dashboard);
        app.back();
        assert_eq!(app.state, AppState::Startup);
        assert!(app.back().is_none());
    }

    // --- Status messages ---

    #[test]
    fn set_status_stores_message() {
        let mut app = App::new();
        app.set_status("hello", 1000);
        assert_eq!(app.status_message(), Some("hello"));
    }

    #[test]
    fn clear_expired_removes_old_message() {
        let mut app = App::new();
        app.set_status("old", 1000);
        app.clear_expired_status(7000); // 6000ms later, TTL is 5000
        assert!(app.status_message().is_none());
    }

    #[test]
    fn clear_expired_keeps_fresh_message() {
        let mut app = App::new();
        app.set_status("fresh", 1000);
        app.clear_expired_status(2000); // only 1000ms later
        assert_eq!(app.status_message(), Some("fresh"));
    }

    #[test]
    fn clear_status_removes_immediately() {
        let mut app = App::new();
        app.set_status("msg", 1000);
        app.clear_status();
        assert!(app.status_message().is_none());
    }

    #[test]
    fn status_message_none_when_unset() {
        let app = App::new();
        assert!(app.status_message().is_none());
    }

    #[test]
    fn clear_expired_with_no_message() {
        let mut app = App::new();
        app.clear_expired_status(9999); // should not panic
        assert!(app.status_message().is_none());
    }

    // --- Refresh timing ---

    #[test]
    fn needs_refresh_initially() {
        let app = App::new();
        assert!(app.needs_refresh(3000));
    }

    #[test]
    fn needs_refresh_after_interval() {
        let mut app = App::new();
        app.mark_refreshed(1000);
        assert!(!app.needs_refresh(2000)); // 1000ms, interval=2000
        assert!(app.needs_refresh(3000));  // 2000ms elapsed
    }

    #[test]
    fn mark_refreshed_updates_timestamp() {
        let mut app = App::new();
        app.mark_refreshed(5000);
        assert!(!app.needs_refresh(5000));
    }

    // --- Key handling: view mode ---

    #[test]
    fn quit_key() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Char('q'));
        assert_eq!(action, Some(AppAction::Quit));
    }

    #[test]
    fn help_key() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Char('?'));
        assert!(matches!(action, Some(AppAction::Navigate(AppState::HelpView { .. }))));
    }

    #[test]
    fn refresh_key() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Char('r'));
        assert_eq!(action, Some(AppAction::Refresh));
    }

    #[test]
    fn select_next_j_key() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Char('j'));
        assert_eq!(action, Some(AppAction::SelectNext));
    }

    #[test]
    fn select_prev_k_key() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Char('k'));
        assert_eq!(action, Some(AppAction::SelectPrev));
    }

    #[test]
    fn slash_enters_command_mode() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Char('/'));
        assert!(action.is_none()); // transitions, no action
        assert_eq!(app.state, AppState::CommandEntry);
    }

    #[test]
    fn colon_enters_command_mode() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        app.handle_key(Key::Char(':'));
        assert_eq!(app.state, AppState::CommandEntry);
    }

    #[test]
    fn escape_in_view_goes_back() {
        let mut app = App::new();
        app.transition(AppState::Dashboard);
        app.transition(AppState::ConfigView);
        app.handle_key(Key::Escape);
        assert_eq!(app.state, AppState::Dashboard);
    }

    #[test]
    fn escape_in_view_no_stack_cancels() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Escape);
        assert_eq!(action, Some(AppAction::Cancel));
    }

    #[test]
    fn down_arrow_selects_next() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Down);
        assert_eq!(action, Some(AppAction::SelectNext));
    }

    #[test]
    fn up_arrow_selects_prev() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::Up);
        assert_eq!(action, Some(AppAction::SelectPrev));
    }

    #[test]
    fn page_down_scrolls_down() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::PageDown);
        assert_eq!(action, Some(AppAction::ScrollDown));
    }

    #[test]
    fn page_up_scrolls_up() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::PageUp);
        assert_eq!(action, Some(AppAction::ScrollUp));
    }

    // --- Key handling: command entry mode ---

    #[test]
    fn command_escape_cancels() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        let action = app.handle_key(Key::Escape);
        assert_eq!(action, Some(AppAction::Cancel));
        assert!(app.input.is_empty());
    }

    #[test]
    fn command_enter_empty_cancels() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        let action = app.handle_key(Key::Enter);
        assert_eq!(action, Some(AppAction::Cancel));
    }

    #[test]
    fn command_enter_with_text_sends() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        app.input.insert('s');
        app.input.insert('t');
        let action = app.handle_key(Key::Enter);
        assert!(matches!(action, Some(AppAction::SendCommand(ref s)) if s == "st"));
    }

    #[test]
    fn command_char_inserts() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        app.handle_key(Key::Char('a'));
        app.handle_key(Key::Char('b'));
        assert_eq!(app.input.text(), "ab");
    }

    #[test]
    fn command_backspace_deletes() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        app.input.insert('x');
        app.handle_key(Key::Backspace);
        assert!(app.input.is_empty());
    }

    #[test]
    fn command_ctrl_u_clears() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        app.input.insert('a');
        app.input.insert('b');
        app.handle_key(Key::Ctrl('u'));
        assert!(app.input.is_empty());
    }

    #[test]
    fn command_ctrl_a_home() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        app.input.insert('a');
        app.input.insert('b');
        app.handle_key(Key::Ctrl('a'));
        assert_eq!(app.input.cursor_pos(), 0);
    }

    #[test]
    fn command_ctrl_e_end() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        app.input.insert('a');
        app.input.insert('b');
        app.input.move_home();
        app.handle_key(Key::Ctrl('e'));
        assert_eq!(app.input.cursor_pos(), 2);
    }

    // --- Key handling: confirm mode ---

    #[test]
    fn confirm_y_confirms() {
        let mut app = App::new();
        app.state = AppState::Confirm {
            prompt: "really?".into(),
            action: PendingAction::KillAgent { name: "w1".into() },
        };
        let action = app.handle_key(Key::Char('y'));
        assert_eq!(action, Some(AppAction::Confirm));
    }

    #[test]
    fn confirm_n_cancels() {
        let mut app = App::new();
        app.state = AppState::Confirm {
            prompt: "really?".into(),
            action: PendingAction::KillAgent { name: "w1".into() },
        };
        let action = app.handle_key(Key::Char('n'));
        assert_eq!(action, Some(AppAction::Cancel));
    }

    #[test]
    fn confirm_enter_confirms() {
        let mut app = App::new();
        app.state = AppState::Confirm {
            prompt: "ok?".into(),
            action: PendingAction::RestartAgent { name: "w1".into() },
        };
        let action = app.handle_key(Key::Enter);
        assert_eq!(action, Some(AppAction::Confirm));
    }

    #[test]
    fn confirm_escape_cancels() {
        let mut app = App::new();
        app.state = AppState::Confirm {
            prompt: "sure?".into(),
            action: PendingAction::CancelTask { id: "T1".into() },
        };
        let action = app.handle_key(Key::Escape);
        assert_eq!(action, Some(AppAction::Cancel));
    }

    #[test]
    fn confirm_other_key_ignored() {
        let mut app = App::new();
        app.state = AppState::Confirm {
            prompt: "ok?".into(),
            action: PendingAction::KillAgent { name: "w1".into() },
        };
        let action = app.handle_key(Key::Char('x'));
        assert!(action.is_none());
    }

    // --- Selection helpers ---

    #[test]
    fn select_next_increments() {
        let mut app = App::new();
        app.select_next(5);
        assert_eq!(app.selected_index, 1);
        app.select_next(5);
        assert_eq!(app.selected_index, 2);
    }

    #[test]
    fn select_next_clamps_at_max() {
        let mut app = App::new();
        app.selected_index = 5;
        app.select_next(5);
        assert_eq!(app.selected_index, 5);
    }

    #[test]
    fn select_prev_decrements() {
        let mut app = App::new();
        app.selected_index = 3;
        app.select_prev();
        assert_eq!(app.selected_index, 2);
    }

    #[test]
    fn select_prev_clamps_at_zero() {
        let mut app = App::new();
        app.select_prev();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn scroll_up_decrements() {
        let mut app = App::new();
        app.scroll_offset = 5;
        app.scroll_up();
        assert_eq!(app.scroll_offset, 4);
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let mut app = App::new();
        app.scroll_up();
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn scroll_down_increments() {
        let mut app = App::new();
        app.scroll_down(10);
        assert_eq!(app.scroll_offset, 1);
    }

    #[test]
    fn scroll_down_clamps_at_max() {
        let mut app = App::new();
        app.scroll_offset = 10;
        app.scroll_down(10);
        assert_eq!(app.scroll_offset, 10);
    }

    #[test]
    fn page_up_subtracts() {
        let mut app = App::new();
        app.scroll_offset = 20;
        app.page_up(10);
        assert_eq!(app.scroll_offset, 10);
    }

    #[test]
    fn page_up_clamps_at_zero() {
        let mut app = App::new();
        app.scroll_offset = 3;
        app.page_up(10);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn page_down_adds() {
        let mut app = App::new();
        app.page_down(10, 50);
        assert_eq!(app.scroll_offset, 10);
    }

    #[test]
    fn page_down_clamps_at_max() {
        let mut app = App::new();
        app.scroll_offset = 45;
        app.page_down(10, 50);
        assert_eq!(app.scroll_offset, 50);
    }

    // --- handle_enter ---

    #[test]
    fn handle_enter_empty_returns_none() {
        let mut app = App::new();
        assert!(app.handle_enter().is_none());
    }

    #[test]
    fn handle_enter_with_text_returns_text() {
        let mut app = App::new();
        app.input.insert('h');
        app.input.insert('i');
        let result = app.handle_enter();
        assert_eq!(result, Some("hi".into()));
        assert!(app.input.is_empty()); // cleared after submit
    }

    // --- AppState labels ---

    #[test]
    fn state_labels() {
        assert_eq!(AppState::Startup.label(), "startup");
        assert_eq!(AppState::Dashboard.label(), "dashboard");
        assert_eq!(AppState::AgentDetail { name: "w1".into() }.label(), "agent");
        assert_eq!(AppState::TaskDetail { id: "T1".into() }.label(), "task");
        assert_eq!(AppState::ConfigView.label(), "config");
        assert_eq!(AppState::LogView.label(), "log");
        assert_eq!(AppState::HelpView { topic: None }.label(), "help");
        assert_eq!(AppState::CommandEntry.label(), "command");
        assert_eq!(
            AppState::Confirm {
                prompt: "ok?".into(),
                action: PendingAction::KillAgent { name: "w1".into() }
            }.label(),
            "confirm"
        );
    }

    // --- PendingAction descriptions ---

    #[test]
    fn pending_action_descriptions() {
        assert!(PendingAction::KillAgent { name: "w1".into() }
            .description()
            .contains("Kill"));
        assert!(PendingAction::RestartAgent { name: "w1".into() }
            .description()
            .contains("Restart"));
        assert!(PendingAction::CancelTask { id: "T1".into() }
            .description()
            .contains("Cancel"));
        assert!(PendingAction::RemoveProject { name: "proj".into() }
            .description()
            .contains("Remove"));
        assert!(PendingAction::Custom {
            description: "custom desc".into(),
            command_json: "{}".into(),
        }
        .description()
        .contains("custom desc"));
    }

    // --- Misc ---

    #[test]
    fn unhandled_view_key_returns_none() {
        let mut app = App::new();
        app.navigate_to(AppState::Dashboard);
        let action = app.handle_key(Key::F(12));
        assert!(action.is_none());
    }

    #[test]
    fn command_tab_completes() {
        let mut app = App::new();
        app.state = AppState::CommandEntry;
        // Type "statu" and tab should complete to "status "
        for ch in "statu".chars() {
            app.input.insert(ch);
        }
        app.handle_tab();
        assert!(app.input.text().starts_with("status"));
    }
}
