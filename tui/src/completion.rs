//! Tab completion for CMX commands.
//!
//! Provides a [`Completer`] that knows the full CMX command tree and can
//! suggest completions for partial input. Supports both static completions
//! (command names, fixed argument values) and dynamic completion types
//! that can be resolved at runtime.

/// Specifies how arguments for a command can be completed.
#[derive(Debug, Clone)]
pub enum ArgCompletions {
    /// No completion available.
    None,
    /// A fixed set of allowed values.
    Fixed(Vec<String>),
    /// A dynamic source resolved at runtime (e.g., "agents", "tasks").
    Dynamic(String),
}


/// Specification of a single argument to a command.
#[derive(Debug, Clone)]
pub struct ArgSpec {
    pub name: String,
    pub required: bool,
    pub completions: ArgCompletions,
}


/// A completable command entry in the command tree.
#[derive(Debug, Clone)]
pub struct CompletionEntry {
    /// The command prefix tokens (e.g., `["agent", "new"]` for `agent.new`).
    pub prefix: Vec<String>,
    /// Human-readable description.
    pub description: String,
    /// Argument specifications.
    pub args: Vec<ArgSpec>,
}


/// Result of a completion attempt.
#[derive(Debug, Clone)]
pub struct CompletionResult {
    /// All matching candidates.
    pub candidates: Vec<String>,
    /// The longest common prefix among candidates.
    pub common_prefix: String,
    /// True if there is exactly one candidate and the input matches it fully.
    pub complete: bool,
}


/// Tab completer for CMX commands.
pub struct Completer {
    commands: Vec<CompletionEntry>,
}


impl Completer {
    /// Create an empty completer.
    pub fn new() -> Self {
        Completer {
            commands: Vec::new(),
        }
    }

    /// Create a completer pre-loaded with the standard CMX command tree.
    pub fn with_default_commands() -> Self {
        let mut c = Completer::new();
        for entry in build_command_tree() {
            c.commands.push(entry);
        }
        c
    }

    /// Add a completion entry.
    pub fn add_entry(&mut self, entry: CompletionEntry) {
        self.commands.push(entry);
    }

    /// Attempt completion at the given cursor position in the input string.
    ///
    /// Returns a [`CompletionResult`] with matching candidates and their
    /// common prefix.
    pub fn complete(&self, input: &str, cursor_pos: usize) -> CompletionResult {
        let text = &input[..cursor_pos.min(input.len())];
        let trimmed = text.trim_start();

        if trimmed.is_empty() {
            // Complete from all top-level command words
            let tops = self.top_level_words();
            let common = longest_common_prefix(&tops);
            return CompletionResult {
                candidates: tops,
                common_prefix: common,
                complete: false,
            };
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let trailing_space = text.ends_with(' ');

        if parts.is_empty() {
            return CompletionResult {
                candidates: vec![],
                common_prefix: String::new(),
                complete: false,
            };
        }

        // Determine what we are completing: a command token or an argument
        let first = parts[0];

        // Check if we're completing the first token (command name)
        if parts.len() == 1 && !trailing_space {
            return self.complete_command_token(first);
        }

        // We have a command — try to find matching entries
        let matching_entries = self.find_matching_entries(first);

        if matching_entries.is_empty() {
            return CompletionResult {
                candidates: vec![],
                common_prefix: String::new(),
                complete: false,
            };
        }

        // Check if any matching entries have two-part prefixes (subcommands)
        let has_subcommands = matching_entries.iter().any(|e| e.prefix.len() >= 2);

        // If the command has subcommands and we're ready to complete the second token
        if has_subcommands {
            // "agent " with trailing space -> complete subcommand from empty
            if parts.len() == 1 && trailing_space {
                return self.complete_subcommand(first, "", &matching_entries);
            }
            // "agent n" without trailing space -> complete partial subcommand
            if parts.len() == 2 && !trailing_space {
                let second = parts[1];
                return self.complete_subcommand(first, second, &matching_entries);
            }
        }

        // Otherwise, complete arguments
        // Find the exact entry for this command
        let entry = if parts.len() >= 2 {
            let sub = parts[1];
            matching_entries
                .iter()
                .find(|e| e.prefix.len() == 2 && e.prefix[1] == sub)
                .or_else(|| {
                    matching_entries
                        .iter()
                        .find(|e| e.prefix.len() == 1)
                })
        } else {
            matching_entries
                .iter()
                .find(|e| e.prefix.len() == 1)
                .or_else(|| matching_entries.first())
        };

        if let Some(entry) = entry {
            let cmd_token_count = entry.prefix.len();
            if parts.len() >= cmd_token_count {
                let arg_pos = if trailing_space {
                    parts.len() - cmd_token_count
                } else {
                    (parts.len() - 1).saturating_sub(cmd_token_count)
                };

                if arg_pos < entry.args.len() {
                    let arg = &entry.args[arg_pos];
                    let partial = if trailing_space {
                        ""
                    } else {
                        parts.last().unwrap_or(&"")
                    };
                    return complete_arg(arg, partial);
                }
            }
        }

        CompletionResult {
            candidates: vec![],
            common_prefix: String::new(),
            complete: false,
        }
    }

    /// Complete a partial first token against all command names.
    fn complete_command_token(&self, partial: &str) -> CompletionResult {
        let mut candidates: Vec<String> = Vec::new();

        for entry in &self.commands {
            let cmd_str = entry.prefix.join(".");
            if cmd_str.starts_with(partial) {
                // Offer the first word of the command
                let first = &entry.prefix[0];
                if first.starts_with(partial) && !candidates.contains(first) {
                    candidates.push(first.clone());
                }
            }
            // Also match the dot-separated form
            if entry.prefix.len() == 1 && entry.prefix[0].starts_with(partial) {
                if !candidates.contains(&entry.prefix[0]) {
                    candidates.push(entry.prefix[0].clone());
                }
            }
        }

        candidates.sort();
        candidates.dedup();

        let common = longest_common_prefix(&candidates);
        let complete = candidates.len() == 1 && common == candidates[0];

        CompletionResult {
            candidates,
            common_prefix: common,
            complete,
        }
    }

    /// Complete a subcommand (second token) given the first token.
    fn complete_subcommand(
        &self,
        first: &str,
        partial: &str,
        entries: &[&CompletionEntry],
    ) -> CompletionResult {
        let mut candidates: Vec<String> = Vec::new();

        for entry in entries {
            if entry.prefix.len() >= 2 && entry.prefix[0] == first {
                if entry.prefix[1].starts_with(partial) {
                    candidates.push(entry.prefix[1].clone());
                }
            }
        }

        candidates.sort();
        candidates.dedup();

        let common = longest_common_prefix(&candidates);
        let complete = candidates.len() == 1 && common == candidates[0];

        CompletionResult {
            candidates,
            common_prefix: common,
            complete,
        }
    }

    /// Find all entries whose first prefix token matches the given word.
    fn find_matching_entries(&self, first: &str) -> Vec<&CompletionEntry> {
        self.commands
            .iter()
            .filter(|e| e.prefix[0] == first)
            .collect()
    }

    /// Return all unique top-level command words.
    fn top_level_words(&self) -> Vec<String> {
        let mut words: Vec<String> = self
            .commands
            .iter()
            .map(|e| e.prefix[0].clone())
            .collect();
        words.sort();
        words.dedup();
        words
    }
}


impl Default for Completer {
    fn default() -> Self {
        Self::new()
    }
}


/// Complete an argument against its ArgSpec.
fn complete_arg(arg: &ArgSpec, partial: &str) -> CompletionResult {
    match &arg.completions {
        ArgCompletions::None | ArgCompletions::Dynamic(_) => CompletionResult {
            candidates: vec![],
            common_prefix: String::new(),
            complete: false,
        },
        ArgCompletions::Fixed(values) => {
            let candidates: Vec<String> = values
                .iter()
                .filter(|v| v.starts_with(partial))
                .cloned()
                .collect();
            let common = longest_common_prefix(&candidates);
            let complete = candidates.len() == 1 && common == candidates[0];
            CompletionResult {
                candidates,
                common_prefix: common,
                complete,
            }
        }
    }
}


/// Compute the longest common prefix of a list of strings.
fn longest_common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    if strings.len() == 1 {
        return strings[0].clone();
    }

    let first = &strings[0];
    let mut prefix_len = first.len();

    for s in &strings[1..] {
        prefix_len = prefix_len.min(s.len());
        for (i, (a, b)) in first.chars().zip(s.chars()).enumerate() {
            if a != b {
                prefix_len = prefix_len.min(i);
                break;
            }
        }
    }

    first.chars().take(prefix_len).collect()
}


/// Build the standard CMX command tree for tab completion.
fn build_command_tree() -> Vec<CompletionEntry> {
    let role_values = vec![
        "worker".to_string(),
        "pilot".to_string(),
        "pm".to_string(),
        "curator".to_string(),
        "copilot".to_string(),
    ];

    let format_values = vec!["json".to_string(), "table".to_string()];

    let agent_type_values = vec![
        "claude".to_string(),
        "console".to_string(),
        "ssh".to_string(),
    ];

    let task_status_values = vec![
        "pending".to_string(),
        "in_progress".to_string(),
        "completed".to_string(),
        "failed".to_string(),
        "paused".to_string(),
        "cancelled".to_string(),
    ];

    vec![
        // Top-level
        CompletionEntry {
            prefix: vec!["status".into()],
            description: "Show system status summary".into(),
            args: vec![],
        },
        CompletionEntry {
            prefix: vec!["view".into()],
            description: "Look up an entity by name".into(),
            args: vec![ArgSpec {
                name: "name".into(),
                required: true,
                completions: ArgCompletions::Dynamic("entities".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["help".into()],
            description: "Show help text".into(),
            args: vec![ArgSpec {
                name: "topic".into(),
                required: false,
                completions: ArgCompletions::Fixed(vec![
                    "agent".into(),
                    "task".into(),
                    "project".into(),
                    "config".into(),
                    "layout".into(),
                    "tell".into(),
                    "interrupt".into(),
                ]),
            }],
        },
        // Agent commands
        CompletionEntry {
            prefix: vec!["agent".into(), "new".into()],
            description: "Create a new agent".into(),
            args: vec![
                ArgSpec {
                    name: "role".into(),
                    required: true,
                    completions: ArgCompletions::Fixed(role_values.clone()),
                },
                ArgSpec {
                    name: "name".into(),
                    required: false,
                    completions: ArgCompletions::None,
                },
                ArgSpec {
                    name: "agent_type".into(),
                    required: false,
                    completions: ArgCompletions::Fixed(agent_type_values),
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["agent".into(), "kill".into()],
            description: "Kill an agent".into(),
            args: vec![ArgSpec {
                name: "name".into(),
                required: true,
                completions: ArgCompletions::Dynamic("agents".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["agent".into(), "restart".into()],
            description: "Restart an agent".into(),
            args: vec![ArgSpec {
                name: "name".into(),
                required: true,
                completions: ArgCompletions::Dynamic("agents".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["agent".into(), "assign".into()],
            description: "Assign an agent to a task".into(),
            args: vec![
                ArgSpec {
                    name: "name".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("agents".into()),
                },
                ArgSpec {
                    name: "task".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("tasks".into()),
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["agent".into(), "unassign".into()],
            description: "Remove task assignment from an agent".into(),
            args: vec![ArgSpec {
                name: "name".into(),
                required: true,
                completions: ArgCompletions::Dynamic("agents".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["agent".into(), "status".into()],
            description: "Update agent status notes".into(),
            args: vec![
                ArgSpec {
                    name: "name".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("agents".into()),
                },
                ArgSpec {
                    name: "notes".into(),
                    required: false,
                    completions: ArgCompletions::None,
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["agent".into(), "list".into()],
            description: "List all agents".into(),
            args: vec![ArgSpec {
                name: "format".into(),
                required: false,
                completions: ArgCompletions::Fixed(format_values.clone()),
            }],
        },
        // Task commands
        CompletionEntry {
            prefix: vec!["task".into(), "list".into()],
            description: "List all tasks".into(),
            args: vec![ArgSpec {
                name: "format".into(),
                required: false,
                completions: ArgCompletions::Fixed(format_values.clone()),
            }],
        },
        CompletionEntry {
            prefix: vec!["task".into(), "get".into()],
            description: "Get task details".into(),
            args: vec![ArgSpec {
                name: "id".into(),
                required: true,
                completions: ArgCompletions::Dynamic("tasks".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["task".into(), "set".into()],
            description: "Update task fields".into(),
            args: vec![
                ArgSpec {
                    name: "id".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("tasks".into()),
                },
                ArgSpec {
                    name: "status".into(),
                    required: false,
                    completions: ArgCompletions::Fixed(task_status_values),
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["task".into(), "check".into()],
            description: "Mark task as completed".into(),
            args: vec![ArgSpec {
                name: "id".into(),
                required: true,
                completions: ArgCompletions::Dynamic("tasks".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["task".into(), "uncheck".into()],
            description: "Mark task as pending".into(),
            args: vec![ArgSpec {
                name: "id".into(),
                required: true,
                completions: ArgCompletions::Dynamic("tasks".into()),
            }],
        },
        // Config commands
        CompletionEntry {
            prefix: vec!["config".into(), "load".into()],
            description: "Load settings from YAML".into(),
            args: vec![ArgSpec {
                name: "path".into(),
                required: false,
                completions: ArgCompletions::None,
            }],
        },
        CompletionEntry {
            prefix: vec!["config".into(), "save".into()],
            description: "Save settings to YAML".into(),
            args: vec![ArgSpec {
                name: "path".into(),
                required: false,
                completions: ArgCompletions::None,
            }],
        },
        CompletionEntry {
            prefix: vec!["config".into(), "add".into()],
            description: "Set a configuration value".into(),
            args: vec![
                ArgSpec {
                    name: "key".into(),
                    required: true,
                    completions: ArgCompletions::Fixed(vec![
                        "health_check_interval".into(),
                        "heartbeat_timeout".into(),
                        "message_timeout".into(),
                        "snapshot_interval".into(),
                        "project_root".into(),
                        "max_retries".into(),
                        "backoff_strategy".into(),
                    ]),
                },
                ArgSpec {
                    name: "value".into(),
                    required: true,
                    completions: ArgCompletions::None,
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["config".into(), "list".into()],
            description: "List configuration values".into(),
            args: vec![],
        },
        // Project commands
        CompletionEntry {
            prefix: vec!["project".into(), "add".into()],
            description: "Register a project folder".into(),
            args: vec![
                ArgSpec {
                    name: "name".into(),
                    required: true,
                    completions: ArgCompletions::None,
                },
                ArgSpec {
                    name: "path".into(),
                    required: true,
                    completions: ArgCompletions::None,
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["project".into(), "remove".into()],
            description: "Remove a registered project".into(),
            args: vec![ArgSpec {
                name: "name".into(),
                required: true,
                completions: ArgCompletions::Dynamic("projects".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["project".into(), "list".into()],
            description: "List registered projects".into(),
            args: vec![ArgSpec {
                name: "format".into(),
                required: false,
                completions: ArgCompletions::Fixed(format_values),
            }],
        },
        CompletionEntry {
            prefix: vec!["project".into(), "scan".into()],
            description: "Scan project for task folders".into(),
            args: vec![ArgSpec {
                name: "name".into(),
                required: true,
                completions: ArgCompletions::Dynamic("projects".into()),
            }],
        },
        // Messaging
        CompletionEntry {
            prefix: vec!["tell".into()],
            description: "Send a message to an agent".into(),
            args: vec![
                ArgSpec {
                    name: "agent".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("agents".into()),
                },
                ArgSpec {
                    name: "text".into(),
                    required: true,
                    completions: ArgCompletions::None,
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["interrupt".into()],
            description: "Interrupt an agent".into(),
            args: vec![
                ArgSpec {
                    name: "agent".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("agents".into()),
                },
                ArgSpec {
                    name: "text".into(),
                    required: false,
                    completions: ArgCompletions::None,
                },
            ],
        },
        // Layout commands
        CompletionEntry {
            prefix: vec!["layout".into(), "row".into()],
            description: "Split session horizontally".into(),
            args: vec![ArgSpec {
                name: "session".into(),
                required: true,
                completions: ArgCompletions::Dynamic("sessions".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["layout".into(), "column".into()],
            description: "Split session vertically".into(),
            args: vec![ArgSpec {
                name: "session".into(),
                required: true,
                completions: ArgCompletions::Dynamic("sessions".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["layout".into(), "merge".into()],
            description: "Merge all panes in a session".into(),
            args: vec![ArgSpec {
                name: "session".into(),
                required: true,
                completions: ArgCompletions::Dynamic("sessions".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["layout".into(), "place".into()],
            description: "Place agent in a pane".into(),
            args: vec![
                ArgSpec {
                    name: "pane".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("panes".into()),
                },
                ArgSpec {
                    name: "agent".into(),
                    required: true,
                    completions: ArgCompletions::Dynamic("agents".into()),
                },
            ],
        },
        CompletionEntry {
            prefix: vec!["layout".into(), "capture".into()],
            description: "Capture pane contents".into(),
            args: vec![ArgSpec {
                name: "session".into(),
                required: true,
                completions: ArgCompletions::Dynamic("sessions".into()),
            }],
        },
        CompletionEntry {
            prefix: vec!["layout".into(), "session".into()],
            description: "Create a new tmux session".into(),
            args: vec![
                ArgSpec {
                    name: "name".into(),
                    required: true,
                    completions: ArgCompletions::None,
                },
                ArgSpec {
                    name: "cwd".into(),
                    required: false,
                    completions: ArgCompletions::None,
                },
            ],
        },
        // Client
        CompletionEntry {
            prefix: vec!["client".into(), "next".into()],
            description: "Switch to next view".into(),
            args: vec![],
        },
        CompletionEntry {
            prefix: vec!["client".into(), "prev".into()],
            description: "Switch to previous view".into(),
            args: vec![],
        },
    ]
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_completer() -> Completer {
        Completer::with_default_commands()
    }

    #[test]
    fn empty_input_shows_top_level() {
        let c = make_completer();
        let result = c.complete("", 0);
        assert!(!result.candidates.is_empty());
        assert!(result.candidates.contains(&"status".to_string()));
        assert!(result.candidates.contains(&"agent".to_string()));
        assert!(result.candidates.contains(&"task".to_string()));
        assert!(result.candidates.contains(&"help".to_string()));
    }

    #[test]
    fn partial_command_completion() {
        let c = make_completer();
        let result = c.complete("sta", 3);
        assert!(result.candidates.contains(&"status".to_string()));
        assert_eq!(result.common_prefix, "status");
    }

    #[test]
    fn agent_subcommands() {
        let c = make_completer();
        let result = c.complete("agent ", 6);
        assert!(result.candidates.contains(&"new".to_string()));
        assert!(result.candidates.contains(&"kill".to_string()));
        assert!(result.candidates.contains(&"list".to_string()));
        assert!(result.candidates.contains(&"restart".to_string()));
        assert!(result.candidates.contains(&"assign".to_string()));
        assert!(result.candidates.contains(&"unassign".to_string()));
        assert!(result.candidates.contains(&"status".to_string()));
    }

    #[test]
    fn partial_subcommand() {
        let c = make_completer();
        let result = c.complete("agent n", 7);
        assert!(result.candidates.contains(&"new".to_string()));
        assert_eq!(result.common_prefix, "new");
        assert!(result.complete);
    }

    #[test]
    fn task_subcommands() {
        let c = make_completer();
        let result = c.complete("task ", 5);
        assert!(result.candidates.contains(&"list".to_string()));
        assert!(result.candidates.contains(&"get".to_string()));
        assert!(result.candidates.contains(&"set".to_string()));
        assert!(result.candidates.contains(&"check".to_string()));
        assert!(result.candidates.contains(&"uncheck".to_string()));
    }

    #[test]
    fn config_subcommands() {
        let c = make_completer();
        let result = c.complete("config ", 7);
        assert!(result.candidates.contains(&"load".to_string()));
        assert!(result.candidates.contains(&"save".to_string()));
        assert!(result.candidates.contains(&"add".to_string()));
        assert!(result.candidates.contains(&"list".to_string()));
    }

    #[test]
    fn project_subcommands() {
        let c = make_completer();
        let result = c.complete("project ", 8);
        assert!(result.candidates.contains(&"add".to_string()));
        assert!(result.candidates.contains(&"remove".to_string()));
        assert!(result.candidates.contains(&"list".to_string()));
        assert!(result.candidates.contains(&"scan".to_string()));
    }

    #[test]
    fn layout_subcommands() {
        let c = make_completer();
        let result = c.complete("layout ", 7);
        assert!(result.candidates.contains(&"row".to_string()));
        assert!(result.candidates.contains(&"column".to_string()));
        assert!(result.candidates.contains(&"merge".to_string()));
        assert!(result.candidates.contains(&"place".to_string()));
        assert!(result.candidates.contains(&"capture".to_string()));
        assert!(result.candidates.contains(&"session".to_string()));
    }

    #[test]
    fn client_subcommands() {
        let c = make_completer();
        let result = c.complete("client ", 7);
        assert!(result.candidates.contains(&"next".to_string()));
        assert!(result.candidates.contains(&"prev".to_string()));
    }

    #[test]
    fn help_topic_completion() {
        let c = make_completer();
        let result = c.complete("help ", 5);
        assert!(result.candidates.contains(&"agent".to_string()));
        assert!(result.candidates.contains(&"task".to_string()));
        assert!(result.candidates.contains(&"project".to_string()));
    }

    #[test]
    fn help_partial_topic() {
        let c = make_completer();
        let result = c.complete("help ag", 7);
        assert!(result.candidates.contains(&"agent".to_string()));
        assert!(result.complete);
    }

    #[test]
    fn agent_new_role_completion() {
        let c = make_completer();
        let result = c.complete("agent new ", 10);
        assert!(result.candidates.contains(&"worker".to_string()));
        assert!(result.candidates.contains(&"pilot".to_string()));
        assert!(result.candidates.contains(&"pm".to_string()));
    }

    #[test]
    fn agent_new_partial_role() {
        let c = make_completer();
        let result = c.complete("agent new w", 11);
        assert!(result.candidates.contains(&"worker".to_string()));
        assert!(result.complete);
    }

    #[test]
    fn agent_new_role_p_ambiguous() {
        let c = make_completer();
        let result = c.complete("agent new p", 11);
        // "pilot" and "pm" both start with 'p'
        assert!(result.candidates.contains(&"pilot".to_string()));
        assert!(result.candidates.contains(&"pm".to_string()));
        assert!(!result.complete);
        assert_eq!(result.common_prefix, "p");
    }

    #[test]
    fn config_add_key_completion() {
        let c = make_completer();
        let result = c.complete("config add ", 11);
        assert!(result.candidates.contains(&"max_retries".to_string()));
        assert!(result
            .candidates
            .contains(&"health_check_interval".to_string()));
    }

    #[test]
    fn config_add_partial_key() {
        let c = make_completer();
        let result = c.complete("config add max", 14);
        assert!(result.candidates.contains(&"max_retries".to_string()));
        assert!(result.complete);
    }

    #[test]
    fn no_completion_for_unknown_command() {
        let c = make_completer();
        let result = c.complete("bogus ", 6);
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn partial_s_matches_status() {
        let c = make_completer();
        let result = c.complete("s", 1);
        assert!(result.candidates.contains(&"status".to_string()));
    }

    #[test]
    fn partial_t_matches_task_and_tell() {
        let c = make_completer();
        let result = c.complete("t", 1);
        assert!(result.candidates.contains(&"task".to_string()));
        assert!(result.candidates.contains(&"tell".to_string()));
    }

    #[test]
    fn cursor_in_middle_of_input() {
        let c = make_completer();
        // Input is "agent new worker" but cursor is at position 5 ("agent")
        let result = c.complete("agent new worker", 5);
        // Should complete "agent" not the full string
        // At position 5 we've typed "agent" with no trailing space
        assert!(!result.candidates.is_empty());
    }

    #[test]
    fn longest_common_prefix_works() {
        assert_eq!(
            longest_common_prefix(&["abc".into(), "abd".into(), "abe".into()]),
            "ab"
        );
        assert_eq!(
            longest_common_prefix(&["hello".into()]),
            "hello"
        );
        assert_eq!(longest_common_prefix(&[]), "");
        assert_eq!(
            longest_common_prefix(&["abc".into(), "xyz".into()]),
            ""
        );
    }

    #[test]
    fn completer_default_is_empty() {
        let c = Completer::default();
        let result = c.complete("", 0);
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn add_custom_entry() {
        let mut c = Completer::new();
        c.add_entry(CompletionEntry {
            prefix: vec!["custom".into()],
            description: "A custom command".into(),
            args: vec![],
        });
        let result = c.complete("cus", 3);
        assert!(result.candidates.contains(&"custom".to_string()));
    }

    #[test]
    fn completion_result_complete_flag() {
        let c = make_completer();

        // "status" is a unique match
        let result = c.complete("statu", 5);
        assert!(result.candidates.contains(&"status".to_string()));
        assert!(result.complete);

        // "a" is ambiguous (agent)
        let result = c.complete("a", 1);
        // Only "agent" starts with "a" in top-level
        // But there might be others; check
        if result.candidates.len() == 1 {
            assert!(result.complete);
        }
    }

    #[test]
    fn build_command_tree_is_comprehensive() {
        let tree = build_command_tree();
        // We should have at least 30 entries
        assert!(tree.len() >= 30, "Expected >= 30 entries, got {}", tree.len());

        // Check some known entries exist
        let has_status = tree.iter().any(|e| e.prefix == vec!["status"]);
        let has_agent_new = tree
            .iter()
            .any(|e| e.prefix == vec!["agent", "new"]);
        let has_task_list = tree
            .iter()
            .any(|e| e.prefix == vec!["task", "list"]);
        assert!(has_status);
        assert!(has_agent_new);
        assert!(has_task_list);
    }

    #[test]
    fn all_entries_have_nonempty_prefix() {
        let tree = build_command_tree();
        for entry in &tree {
            assert!(!entry.prefix.is_empty());
            for p in &entry.prefix {
                assert!(!p.is_empty());
            }
        }
    }

    #[test]
    fn all_entries_have_description() {
        let tree = build_command_tree();
        for entry in &tree {
            assert!(
                !entry.description.is_empty(),
                "Entry {:?} has empty description",
                entry.prefix
            );
        }
    }
}
