//! Key binding configuration and dispatch.
//!
//! Provides `KeyMap`, the registry of all active key bindings. Each binding
//! maps a key + modifier combination to an `AppAction` within a `BindingContext`.
//! Multiple bindings can target the same action — later bindings override
//! earlier ones for the same key + context combination.

use crate::app::{AppAction, AppState, Key};


// ---------------------------------------------------------------------------
// Modifier
// ---------------------------------------------------------------------------

/// Modifier keys that can accompany a primary key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    None,
    Ctrl,
    Alt,
    Shift,
}


// ---------------------------------------------------------------------------
// BindingContext
// ---------------------------------------------------------------------------

/// The context in which a key binding is active.
///
/// A binding with `Global` context is active in every view. A binding with
/// a view-specific context is only active in that view. View-specific
/// bindings take priority over global ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingContext {
    /// Active in all views.
    Global,
    /// Active only in the Dashboard view.
    Dashboard,
    /// Active only in agent detail views.
    AgentDetail,
    /// Active only in task detail views.
    TaskDetail,
    /// Active only in log views.
    LogView,
    /// Active only in config views.
    ConfigView,
    /// Active only in help views.
    HelpView,
    /// Active only in command entry mode.
    CommandEntry,
    /// Active only in confirm dialogs.
    Confirm,
}

impl BindingContext {
    /// Return whether this context matches the given application state.
    pub fn matches_state(&self, state: &AppState) -> bool {
        match self {
            BindingContext::Global => true,
            BindingContext::Dashboard => matches!(state, AppState::Dashboard),
            BindingContext::AgentDetail => matches!(state, AppState::AgentDetail { .. }),
            BindingContext::TaskDetail => matches!(state, AppState::TaskDetail { .. }),
            BindingContext::LogView => matches!(state, AppState::LogView),
            BindingContext::ConfigView => matches!(state, AppState::ConfigView),
            BindingContext::HelpView => matches!(state, AppState::HelpView { .. }),
            BindingContext::CommandEntry => matches!(state, AppState::CommandEntry),
            BindingContext::Confirm => matches!(state, AppState::Confirm { .. }),
        }
    }

    /// Return a short label for this context, suitable for display.
    pub fn label(&self) -> &str {
        match self {
            BindingContext::Global => "global",
            BindingContext::Dashboard => "dashboard",
            BindingContext::AgentDetail => "agent_detail",
            BindingContext::TaskDetail => "task_detail",
            BindingContext::LogView => "log",
            BindingContext::ConfigView => "config",
            BindingContext::HelpView => "help",
            BindingContext::CommandEntry => "command",
            BindingContext::Confirm => "confirm",
        }
    }
}


// ---------------------------------------------------------------------------
// KeyBinding
// ---------------------------------------------------------------------------

/// A single key binding: a key + modifier combination that triggers an action
/// within a specific context.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    /// The key that triggers this binding.
    pub key: Key,
    /// Modifier required alongside the key.
    pub modifier: Modifier,
    /// The context in which this binding is active.
    pub context: BindingContext,
    /// The action to trigger when the binding fires.
    pub action: AppAction,
    /// Human-readable description of the binding.
    pub description: String,
    /// Whether this is a user-defined override (true) or a built-in default.
    pub is_custom: bool,
}

impl KeyBinding {
    /// Create a new binding.
    pub fn new(
        key: Key,
        modifier: Modifier,
        context: BindingContext,
        action: AppAction,
        description: &str,
    ) -> Self {
        KeyBinding {
            key,
            modifier,
            context,
            action,
            description: description.to_string(),
            is_custom: false,
        }
    }

    /// Create a new custom (user-defined) binding.
    pub fn custom(
        key: Key,
        modifier: Modifier,
        context: BindingContext,
        action: AppAction,
        description: &str,
    ) -> Self {
        KeyBinding {
            key,
            modifier,
            context,
            action,
            description: description.to_string(),
            is_custom: true,
        }
    }

    /// Return a short display string for the key combination, e.g. "Ctrl+Q".
    pub fn key_display(&self) -> String {
        let key_str = match &self.key {
            Key::Char(c) => {
                if c.is_uppercase() {
                    format!("Shift+{}", c.to_lowercase())
                } else {
                    c.to_string()
                }
            }
            Key::Enter => "Enter".to_string(),
            Key::Tab => "Tab".to_string(),
            Key::Escape => "Esc".to_string(),
            Key::Backspace => "Backspace".to_string(),
            Key::Delete => "Del".to_string(),
            Key::Up => "Up".to_string(),
            Key::Down => "Down".to_string(),
            Key::Left => "Left".to_string(),
            Key::Right => "Right".to_string(),
            Key::Home => "Home".to_string(),
            Key::End => "End".to_string(),
            Key::PageUp => "PgUp".to_string(),
            Key::PageDown => "PgDn".to_string(),
            Key::F(n) => format!("F{}", n),
            Key::Ctrl(c) => return format!("Ctrl+{}", c),
            Key::Alt(c) => return format!("Alt+{}", c),
        };

        match self.modifier {
            Modifier::None => key_str,
            Modifier::Ctrl => format!("Ctrl+{}", key_str),
            Modifier::Alt => format!("Alt+{}", key_str),
            Modifier::Shift => format!("Shift+{}", key_str),
        }
    }

    /// Check if this binding matches the given key, modifier, and state.
    pub fn matches(&self, key: &Key, modifier: Modifier, state: &AppState) -> bool {
        self.key == *key && self.modifier == modifier && self.context.matches_state(state)
    }
}


// ---------------------------------------------------------------------------
// KeyMap
// ---------------------------------------------------------------------------

/// Registry of all active key bindings.
///
/// Bindings are evaluated in order: context-specific bindings are checked
/// before global ones. Within the same context level, later-added bindings
/// override earlier ones for the same key + modifier combination.
pub struct KeyMap {
    bindings: Vec<KeyBinding>,
}

impl KeyMap {
    /// Create an empty keymap.
    pub fn new() -> Self {
        KeyMap {
            bindings: Vec::new(),
        }
    }

    /// Create a keymap with default bindings.
    pub fn with_defaults() -> Self {
        let mut km = KeyMap::new();
        km.load_defaults();
        km
    }

    /// Add a binding to the keymap.
    pub fn add(&mut self, binding: KeyBinding) {
        self.bindings.push(binding);
    }

    /// Remove all bindings for a given key + modifier + context combination.
    pub fn remove(&mut self, key: &Key, modifier: Modifier, context: &BindingContext) {
        self.bindings
            .retain(|b| !(&b.key == key && b.modifier == modifier && &b.context == context));
    }

    /// Look up the action for a key event in the given application state.
    ///
    /// Context-specific bindings take priority over global ones. Among
    /// bindings in the same context, the last-added one wins.
    pub fn lookup(&self, key: &Key, modifier: Modifier, state: &AppState) -> Option<&AppAction> {
        // First, look for context-specific bindings (non-global).
        let specific = self
            .bindings
            .iter()
            .rev()
            .find(|b| {
                b.key == *key
                    && b.modifier == modifier
                    && !matches!(b.context, BindingContext::Global)
                    && b.context.matches_state(state)
            });

        if let Some(binding) = specific {
            return Some(&binding.action);
        }

        // Fall back to global bindings.
        self.bindings
            .iter()
            .rev()
            .find(|b| {
                b.key == *key
                    && b.modifier == modifier
                    && matches!(b.context, BindingContext::Global)
            })
            .map(|b| &b.action)
    }

    /// Return all bindings for the given context.
    pub fn bindings_for_context(&self, context: &BindingContext) -> Vec<&KeyBinding> {
        self.bindings
            .iter()
            .filter(|b| &b.context == context)
            .collect()
    }

    /// Return all global bindings.
    pub fn global_bindings(&self) -> Vec<&KeyBinding> {
        self.bindings_for_context(&BindingContext::Global)
    }

    /// Return all bindings.
    pub fn all_bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    /// Return the total number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Return true if the keymap has no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Return all custom (user-defined) bindings.
    pub fn custom_bindings(&self) -> Vec<&KeyBinding> {
        self.bindings.iter().filter(|b| b.is_custom).collect()
    }

    /// Return all built-in (default) bindings.
    pub fn default_bindings(&self) -> Vec<&KeyBinding> {
        self.bindings.iter().filter(|b| !b.is_custom).collect()
    }

    /// Generate help text for all bindings, grouped by context.
    pub fn help_text(&self) -> String {
        let mut lines = Vec::new();
        let contexts = [
            BindingContext::Global,
            BindingContext::Dashboard,
            BindingContext::AgentDetail,
            BindingContext::TaskDetail,
            BindingContext::LogView,
            BindingContext::ConfigView,
            BindingContext::HelpView,
            BindingContext::CommandEntry,
            BindingContext::Confirm,
        ];

        for ctx in &contexts {
            let bindings = self.bindings_for_context(ctx);
            if bindings.is_empty() {
                continue;
            }
            lines.push(format!("[{}]", ctx.label()));
            for binding in bindings {
                lines.push(format!(
                    "  {:12} {}",
                    binding.key_display(),
                    binding.description
                ));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    }

    // -------------------------------------------------------------------
    // Default bindings
    // -------------------------------------------------------------------

    /// Load the default key bindings.
    fn load_defaults(&mut self) {
        // --- Global ---
        self.add(KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit the application",
        ));
        self.add(KeyBinding::new(
            Key::Char('?'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Navigate(AppState::HelpView { topic: None }),
            "Show help",
        ));
        self.add(KeyBinding::new(
            Key::Char('r'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Refresh,
            "Refresh current view",
        ));
        self.add(KeyBinding::new(
            Key::Char('j'),
            Modifier::None,
            BindingContext::Global,
            AppAction::SelectNext,
            "Select next item",
        ));
        self.add(KeyBinding::new(
            Key::Down,
            Modifier::None,
            BindingContext::Global,
            AppAction::SelectNext,
            "Select next item",
        ));
        self.add(KeyBinding::new(
            Key::Char('k'),
            Modifier::None,
            BindingContext::Global,
            AppAction::SelectPrev,
            "Select previous item",
        ));
        self.add(KeyBinding::new(
            Key::Up,
            Modifier::None,
            BindingContext::Global,
            AppAction::SelectPrev,
            "Select previous item",
        ));
        self.add(KeyBinding::new(
            Key::PageDown,
            Modifier::None,
            BindingContext::Global,
            AppAction::ScrollDown,
            "Scroll down",
        ));
        self.add(KeyBinding::new(
            Key::PageUp,
            Modifier::None,
            BindingContext::Global,
            AppAction::ScrollUp,
            "Scroll up",
        ));
        self.add(KeyBinding::new(
            Key::Escape,
            Modifier::None,
            BindingContext::Global,
            AppAction::Cancel,
            "Go back / cancel",
        ));

        // --- Confirm ---
        self.add(KeyBinding::new(
            Key::Char('y'),
            Modifier::None,
            BindingContext::Confirm,
            AppAction::Confirm,
            "Confirm action",
        ));
        self.add(KeyBinding::new(
            Key::Enter,
            Modifier::None,
            BindingContext::Confirm,
            AppAction::Confirm,
            "Confirm action",
        ));
        self.add(KeyBinding::new(
            Key::Char('n'),
            Modifier::None,
            BindingContext::Confirm,
            AppAction::Cancel,
            "Cancel action",
        ));
        self.add(KeyBinding::new(
            Key::Escape,
            Modifier::None,
            BindingContext::Confirm,
            AppAction::Cancel,
            "Cancel action",
        ));
    }
}


impl Default for KeyMap {
    fn default() -> Self {
        Self::with_defaults()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Modifier ---

    #[test]
    fn modifier_equality() {
        assert_eq!(Modifier::None, Modifier::None);
        assert_eq!(Modifier::Ctrl, Modifier::Ctrl);
        assert_ne!(Modifier::None, Modifier::Ctrl);
    }

    // --- BindingContext ---

    #[test]
    fn global_matches_all_states() {
        let ctx = BindingContext::Global;
        assert!(ctx.matches_state(&AppState::Dashboard));
        assert!(ctx.matches_state(&AppState::Startup));
        assert!(ctx.matches_state(&AppState::LogView));
        assert!(ctx.matches_state(&AppState::CommandEntry));
    }

    #[test]
    fn dashboard_matches_only_dashboard() {
        let ctx = BindingContext::Dashboard;
        assert!(ctx.matches_state(&AppState::Dashboard));
        assert!(!ctx.matches_state(&AppState::Startup));
        assert!(!ctx.matches_state(&AppState::LogView));
    }

    #[test]
    fn agent_detail_matches_agent_detail() {
        let ctx = BindingContext::AgentDetail;
        assert!(ctx.matches_state(&AppState::AgentDetail {
            name: "w1".into()
        }));
        assert!(!ctx.matches_state(&AppState::Dashboard));
    }

    #[test]
    fn task_detail_matches_task_detail() {
        let ctx = BindingContext::TaskDetail;
        assert!(ctx.matches_state(&AppState::TaskDetail { id: "T1".into() }));
        assert!(!ctx.matches_state(&AppState::Dashboard));
    }

    #[test]
    fn log_view_matches_log_view() {
        let ctx = BindingContext::LogView;
        assert!(ctx.matches_state(&AppState::LogView));
        assert!(!ctx.matches_state(&AppState::Dashboard));
    }

    #[test]
    fn config_view_matches_config_view() {
        let ctx = BindingContext::ConfigView;
        assert!(ctx.matches_state(&AppState::ConfigView));
        assert!(!ctx.matches_state(&AppState::Dashboard));
    }

    #[test]
    fn help_view_matches_help_view() {
        let ctx = BindingContext::HelpView;
        assert!(ctx.matches_state(&AppState::HelpView { topic: None }));
        assert!(ctx.matches_state(&AppState::HelpView {
            topic: Some("foo".into())
        }));
        assert!(!ctx.matches_state(&AppState::Dashboard));
    }

    #[test]
    fn command_entry_matches_command_entry() {
        let ctx = BindingContext::CommandEntry;
        assert!(ctx.matches_state(&AppState::CommandEntry));
        assert!(!ctx.matches_state(&AppState::Dashboard));
    }

    #[test]
    fn context_labels() {
        assert_eq!(BindingContext::Global.label(), "global");
        assert_eq!(BindingContext::Dashboard.label(), "dashboard");
        assert_eq!(BindingContext::AgentDetail.label(), "agent_detail");
        assert_eq!(BindingContext::TaskDetail.label(), "task_detail");
        assert_eq!(BindingContext::LogView.label(), "log");
        assert_eq!(BindingContext::ConfigView.label(), "config");
        assert_eq!(BindingContext::HelpView.label(), "help");
        assert_eq!(BindingContext::CommandEntry.label(), "command");
        assert_eq!(BindingContext::Confirm.label(), "confirm");
    }

    // --- KeyBinding ---

    #[test]
    fn binding_new_sets_fields() {
        let b = KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit",
        );
        assert_eq!(b.key, Key::Char('q'));
        assert_eq!(b.modifier, Modifier::None);
        assert_eq!(b.action, AppAction::Quit);
        assert_eq!(b.description, "Quit");
        assert!(!b.is_custom);
    }

    #[test]
    fn binding_custom_sets_is_custom() {
        let b = KeyBinding::custom(
            Key::Char('x'),
            Modifier::Ctrl,
            BindingContext::Global,
            AppAction::Quit,
            "Custom quit",
        );
        assert!(b.is_custom);
    }

    #[test]
    fn binding_key_display_simple() {
        let b = KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit",
        );
        assert_eq!(b.key_display(), "q");
    }

    #[test]
    fn binding_key_display_ctrl() {
        let b = KeyBinding::new(
            Key::Char('c'),
            Modifier::Ctrl,
            BindingContext::Global,
            AppAction::Cancel,
            "Cancel",
        );
        assert_eq!(b.key_display(), "Ctrl+c");
    }

    #[test]
    fn binding_key_display_ctrl_key() {
        let b = KeyBinding::new(
            Key::Ctrl('c'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Cancel,
            "Cancel",
        );
        assert_eq!(b.key_display(), "Ctrl+c");
    }

    #[test]
    fn binding_key_display_alt() {
        let b = KeyBinding::new(
            Key::Char('x'),
            Modifier::Alt,
            BindingContext::Global,
            AppAction::Quit,
            "Alt-quit",
        );
        assert_eq!(b.key_display(), "Alt+x");
    }

    #[test]
    fn binding_key_display_function_key() {
        let b = KeyBinding::new(
            Key::F(1),
            Modifier::None,
            BindingContext::Global,
            AppAction::Navigate(AppState::HelpView { topic: None }),
            "Help",
        );
        assert_eq!(b.key_display(), "F1");
    }

    #[test]
    fn binding_key_display_special_keys() {
        let cases: Vec<(Key, &str)> = vec![
            (Key::Enter, "Enter"),
            (Key::Tab, "Tab"),
            (Key::Escape, "Esc"),
            (Key::Backspace, "Backspace"),
            (Key::Delete, "Del"),
            (Key::Up, "Up"),
            (Key::Down, "Down"),
            (Key::Left, "Left"),
            (Key::Right, "Right"),
            (Key::Home, "Home"),
            (Key::End, "End"),
            (Key::PageUp, "PgUp"),
            (Key::PageDown, "PgDn"),
        ];
        for (key, expected) in cases {
            let b = KeyBinding::new(
                key,
                Modifier::None,
                BindingContext::Global,
                AppAction::Refresh,
                "test",
            );
            assert_eq!(b.key_display(), expected);
        }
    }

    #[test]
    fn binding_matches_correct_context() {
        let b = KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Dashboard,
            AppAction::Quit,
            "Quit",
        );
        assert!(b.matches(&Key::Char('q'), Modifier::None, &AppState::Dashboard));
        assert!(!b.matches(&Key::Char('q'), Modifier::None, &AppState::Startup));
    }

    #[test]
    fn binding_matches_modifier() {
        let b = KeyBinding::new(
            Key::Char('c'),
            Modifier::Ctrl,
            BindingContext::Global,
            AppAction::Cancel,
            "Cancel",
        );
        assert!(b.matches(&Key::Char('c'), Modifier::Ctrl, &AppState::Dashboard));
        assert!(!b.matches(&Key::Char('c'), Modifier::None, &AppState::Dashboard));
    }

    #[test]
    fn binding_matches_key() {
        let b = KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit",
        );
        assert!(b.matches(&Key::Char('q'), Modifier::None, &AppState::Dashboard));
        assert!(!b.matches(&Key::Char('x'), Modifier::None, &AppState::Dashboard));
    }

    // --- KeyMap ---

    #[test]
    fn empty_keymap() {
        let km = KeyMap::new();
        assert!(km.is_empty());
        assert_eq!(km.len(), 0);
    }

    #[test]
    fn with_defaults_has_bindings() {
        let km = KeyMap::with_defaults();
        assert!(!km.is_empty());
        assert!(km.len() > 5);
    }

    #[test]
    fn default_impl() {
        let km = KeyMap::default();
        assert!(!km.is_empty());
    }

    #[test]
    fn add_and_lookup() {
        let mut km = KeyMap::new();
        km.add(KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit",
        ));
        let action = km.lookup(&Key::Char('q'), Modifier::None, &AppState::Dashboard);
        assert_eq!(action, Some(&AppAction::Quit));
    }

    #[test]
    fn lookup_miss() {
        let km = KeyMap::new();
        let action = km.lookup(&Key::Char('z'), Modifier::None, &AppState::Dashboard);
        assert!(action.is_none());
    }

    #[test]
    fn context_specific_overrides_global() {
        let mut km = KeyMap::new();
        km.add(KeyBinding::new(
            Key::Char('r'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Refresh,
            "Refresh (global)",
        ));
        km.add(KeyBinding::new(
            Key::Char('r'),
            Modifier::None,
            BindingContext::Dashboard,
            AppAction::SendCommand("status".into()),
            "Status (dashboard)",
        ));

        // In dashboard, context-specific wins.
        let action = km.lookup(&Key::Char('r'), Modifier::None, &AppState::Dashboard);
        assert_eq!(action, Some(&AppAction::SendCommand("status".into())));

        // In log view, global applies.
        let action = km.lookup(&Key::Char('r'), Modifier::None, &AppState::LogView);
        assert_eq!(action, Some(&AppAction::Refresh));
    }

    #[test]
    fn later_binding_overrides_earlier() {
        let mut km = KeyMap::new();
        km.add(KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit first",
        ));
        km.add(KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Refresh,
            "Refresh override",
        ));

        let action = km.lookup(&Key::Char('q'), Modifier::None, &AppState::Dashboard);
        assert_eq!(action, Some(&AppAction::Refresh));
    }

    #[test]
    fn remove_binding() {
        let mut km = KeyMap::new();
        km.add(KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit",
        ));
        assert_eq!(km.len(), 1);

        km.remove(&Key::Char('q'), Modifier::None, &BindingContext::Global);
        assert_eq!(km.len(), 0);
        assert!(km
            .lookup(&Key::Char('q'), Modifier::None, &AppState::Dashboard)
            .is_none());
    }

    #[test]
    fn remove_preserves_other_bindings() {
        let mut km = KeyMap::new();
        km.add(KeyBinding::new(
            Key::Char('q'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Quit,
            "Quit",
        ));
        km.add(KeyBinding::new(
            Key::Char('r'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Refresh,
            "Refresh",
        ));

        km.remove(&Key::Char('q'), Modifier::None, &BindingContext::Global);
        assert_eq!(km.len(), 1);
        assert!(km
            .lookup(&Key::Char('r'), Modifier::None, &AppState::Dashboard)
            .is_some());
    }

    #[test]
    fn bindings_for_context_filters() {
        let km = KeyMap::with_defaults();
        let global = km.bindings_for_context(&BindingContext::Global);
        let confirm = km.bindings_for_context(&BindingContext::Confirm);

        assert!(!global.is_empty());
        assert!(!confirm.is_empty());

        for b in &global {
            assert_eq!(b.context, BindingContext::Global);
        }
        for b in &confirm {
            assert_eq!(b.context, BindingContext::Confirm);
        }
    }

    #[test]
    fn global_bindings_returns_only_global() {
        let km = KeyMap::with_defaults();
        let global = km.global_bindings();
        for b in &global {
            assert_eq!(b.context, BindingContext::Global);
        }
    }

    #[test]
    fn all_bindings_returns_all() {
        let km = KeyMap::with_defaults();
        assert_eq!(km.all_bindings().len(), km.len());
    }

    #[test]
    fn custom_bindings_initially_empty() {
        let km = KeyMap::with_defaults();
        assert!(km.custom_bindings().is_empty());
    }

    #[test]
    fn custom_binding_tracked() {
        let mut km = KeyMap::with_defaults();
        km.add(KeyBinding::custom(
            Key::Char('x'),
            Modifier::Ctrl,
            BindingContext::Global,
            AppAction::Quit,
            "Custom quit",
        ));
        assert_eq!(km.custom_bindings().len(), 1);
        assert!(km.custom_bindings()[0].is_custom);
    }

    #[test]
    fn default_bindings_tracked() {
        let km = KeyMap::with_defaults();
        let defaults = km.default_bindings();
        assert_eq!(defaults.len(), km.len());
        for b in &defaults {
            assert!(!b.is_custom);
        }
    }

    #[test]
    fn help_text_not_empty() {
        let km = KeyMap::with_defaults();
        let text = km.help_text();
        assert!(!text.is_empty());
        assert!(text.contains("[global]"));
        assert!(text.contains("Quit"));
    }

    #[test]
    fn help_text_empty_keymap() {
        let km = KeyMap::new();
        let text = km.help_text();
        assert!(text.is_empty());
    }

    #[test]
    fn defaults_include_quit() {
        let km = KeyMap::with_defaults();
        let action = km.lookup(&Key::Char('q'), Modifier::None, &AppState::Dashboard);
        assert_eq!(action, Some(&AppAction::Quit));
    }

    #[test]
    fn defaults_include_help() {
        let km = KeyMap::with_defaults();
        let action = km.lookup(&Key::Char('?'), Modifier::None, &AppState::Dashboard);
        assert!(matches!(
            action,
            Some(AppAction::Navigate(AppState::HelpView { .. }))
        ));
    }

    #[test]
    fn defaults_include_refresh() {
        let km = KeyMap::with_defaults();
        let action = km.lookup(&Key::Char('r'), Modifier::None, &AppState::Dashboard);
        assert_eq!(action, Some(&AppAction::Refresh));
    }

    #[test]
    fn defaults_include_navigation() {
        let km = KeyMap::with_defaults();
        let next = km.lookup(&Key::Char('j'), Modifier::None, &AppState::Dashboard);
        let prev = km.lookup(&Key::Char('k'), Modifier::None, &AppState::Dashboard);
        assert_eq!(next, Some(&AppAction::SelectNext));
        assert_eq!(prev, Some(&AppAction::SelectPrev));
    }

    #[test]
    fn defaults_include_arrow_navigation() {
        let km = KeyMap::with_defaults();
        let next = km.lookup(&Key::Down, Modifier::None, &AppState::Dashboard);
        let prev = km.lookup(&Key::Up, Modifier::None, &AppState::Dashboard);
        assert_eq!(next, Some(&AppAction::SelectNext));
        assert_eq!(prev, Some(&AppAction::SelectPrev));
    }

    #[test]
    fn defaults_include_page_scroll() {
        let km = KeyMap::with_defaults();
        let down = km.lookup(&Key::PageDown, Modifier::None, &AppState::Dashboard);
        let up = km.lookup(&Key::PageUp, Modifier::None, &AppState::Dashboard);
        assert_eq!(down, Some(&AppAction::ScrollDown));
        assert_eq!(up, Some(&AppAction::ScrollUp));
    }

    #[test]
    fn defaults_confirm_y_confirms() {
        let km = KeyMap::with_defaults();
        let action = km.lookup(
            &Key::Char('y'),
            Modifier::None,
            &AppState::Confirm {
                prompt: "ok?".into(),
                action: crate::app::PendingAction::KillAgent { name: "w1".into() },
            },
        );
        assert_eq!(action, Some(&AppAction::Confirm));
    }

    #[test]
    fn defaults_confirm_n_cancels() {
        let km = KeyMap::with_defaults();
        let action = km.lookup(
            &Key::Char('n'),
            Modifier::None,
            &AppState::Confirm {
                prompt: "ok?".into(),
                action: crate::app::PendingAction::KillAgent { name: "w1".into() },
            },
        );
        assert_eq!(action, Some(&AppAction::Cancel));
    }

    #[test]
    fn modifier_with_different_keys_are_distinct() {
        let mut km = KeyMap::new();
        km.add(KeyBinding::new(
            Key::Char('c'),
            Modifier::Ctrl,
            BindingContext::Global,
            AppAction::Cancel,
            "Cancel",
        ));
        km.add(KeyBinding::new(
            Key::Char('c'),
            Modifier::None,
            BindingContext::Global,
            AppAction::Refresh,
            "Refresh",
        ));

        let ctrl = km.lookup(&Key::Char('c'), Modifier::Ctrl, &AppState::Dashboard);
        let none = km.lookup(&Key::Char('c'), Modifier::None, &AppState::Dashboard);
        assert_eq!(ctrl, Some(&AppAction::Cancel));
        assert_eq!(none, Some(&AppAction::Refresh));
    }

    #[test]
    fn uppercase_key_display() {
        let b = KeyBinding::new(
            Key::Char('G'),
            Modifier::None,
            BindingContext::Global,
            AppAction::ScrollDown,
            "Go to bottom",
        );
        assert!(b.key_display().contains("Shift"));
    }
}
