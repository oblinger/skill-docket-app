//! Theme configuration for the terminal UI.
//!
//! Defines color schemes that control how status indicators, borders,
//! prompts, and other UI elements are colored. Themes are serializable
//! so they can be loaded from configuration files.

use serde::{Deserialize, Serialize};


/// A named color that can be converted to ANSI escape sequences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    Default,
    Red,
    Green,
    Yellow,
    Blue,
    Cyan,
    Magenta,
    White,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    Rgb(u8, u8, u8),
}


impl Color {
    /// Return the ANSI foreground escape sequence for this color.
    pub fn ansi_fg(&self) -> String {
        match self {
            Color::Default => "\x1b[39m".to_string(),
            Color::Red => "\x1b[31m".to_string(),
            Color::Green => "\x1b[32m".to_string(),
            Color::Yellow => "\x1b[33m".to_string(),
            Color::Blue => "\x1b[34m".to_string(),
            Color::Cyan => "\x1b[36m".to_string(),
            Color::Magenta => "\x1b[35m".to_string(),
            Color::White => "\x1b[37m".to_string(),
            Color::BrightRed => "\x1b[91m".to_string(),
            Color::BrightGreen => "\x1b[92m".to_string(),
            Color::BrightYellow => "\x1b[93m".to_string(),
            Color::BrightBlue => "\x1b[94m".to_string(),
            Color::Rgb(r, g, b) => format!("\x1b[38;2;{};{};{}m", r, g, b),
        }
    }

    /// Return the ANSI background escape sequence for this color.
    pub fn ansi_bg(&self) -> String {
        match self {
            Color::Default => "\x1b[49m".to_string(),
            Color::Red => "\x1b[41m".to_string(),
            Color::Green => "\x1b[42m".to_string(),
            Color::Yellow => "\x1b[43m".to_string(),
            Color::Blue => "\x1b[44m".to_string(),
            Color::Cyan => "\x1b[46m".to_string(),
            Color::Magenta => "\x1b[45m".to_string(),
            Color::White => "\x1b[47m".to_string(),
            Color::BrightRed => "\x1b[101m".to_string(),
            Color::BrightGreen => "\x1b[102m".to_string(),
            Color::BrightYellow => "\x1b[103m".to_string(),
            Color::BrightBlue => "\x1b[104m".to_string(),
            Color::Rgb(r, g, b) => format!("\x1b[48;2;{};{};{}m", r, g, b),
        }
    }
}


/// A complete color theme for the MuxUX terminal UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub header_color: Color,
    pub agent_idle: Color,
    pub agent_busy: Color,
    pub agent_error: Color,
    pub agent_dead: Color,
    pub task_pending: Color,
    pub task_active: Color,
    pub task_done: Color,
    pub task_failed: Color,
    pub border: Color,
    pub prompt: Color,
    pub info: Color,
    pub warning: Color,
    pub error: Color,
}


impl Theme {
    /// Dark terminal theme — the default.
    pub fn default_dark() -> Self {
        Theme {
            name: "dark".to_string(),
            header_color: Color::BrightBlue,
            agent_idle: Color::White,
            agent_busy: Color::BrightGreen,
            agent_error: Color::BrightRed,
            agent_dead: Color::Red,
            task_pending: Color::Yellow,
            task_active: Color::Cyan,
            task_done: Color::Green,
            task_failed: Color::Red,
            border: Color::Blue,
            prompt: Color::BrightGreen,
            info: Color::Cyan,
            warning: Color::BrightYellow,
            error: Color::BrightRed,
        }
    }

    /// Light terminal theme.
    pub fn default_light() -> Self {
        Theme {
            name: "light".to_string(),
            header_color: Color::Blue,
            agent_idle: Color::Default,
            agent_busy: Color::Green,
            agent_error: Color::Red,
            agent_dead: Color::Red,
            task_pending: Color::Yellow,
            task_active: Color::Blue,
            task_done: Color::Green,
            task_failed: Color::Red,
            border: Color::Default,
            prompt: Color::Green,
            info: Color::Blue,
            warning: Color::Yellow,
            error: Color::Red,
        }
    }

    /// Minimal theme — no bright colors, only basic ANSI.
    pub fn minimal() -> Self {
        Theme {
            name: "minimal".to_string(),
            header_color: Color::White,
            agent_idle: Color::Default,
            agent_busy: Color::Green,
            agent_error: Color::Red,
            agent_dead: Color::Red,
            task_pending: Color::Default,
            task_active: Color::Default,
            task_done: Color::Green,
            task_failed: Color::Red,
            border: Color::Default,
            prompt: Color::Default,
            info: Color::Default,
            warning: Color::Yellow,
            error: Color::Red,
        }
    }
}


impl Default for Theme {
    fn default() -> Self {
        Theme::default_dark()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_ansi_fg_basic() {
        assert_eq!(Color::Red.ansi_fg(), "\x1b[31m");
        assert_eq!(Color::Green.ansi_fg(), "\x1b[32m");
        assert_eq!(Color::Yellow.ansi_fg(), "\x1b[33m");
        assert_eq!(Color::Blue.ansi_fg(), "\x1b[34m");
        assert_eq!(Color::Cyan.ansi_fg(), "\x1b[36m");
        assert_eq!(Color::Magenta.ansi_fg(), "\x1b[35m");
        assert_eq!(Color::White.ansi_fg(), "\x1b[37m");
        assert_eq!(Color::Default.ansi_fg(), "\x1b[39m");
    }

    #[test]
    fn color_ansi_fg_bright() {
        assert_eq!(Color::BrightRed.ansi_fg(), "\x1b[91m");
        assert_eq!(Color::BrightGreen.ansi_fg(), "\x1b[92m");
        assert_eq!(Color::BrightYellow.ansi_fg(), "\x1b[93m");
        assert_eq!(Color::BrightBlue.ansi_fg(), "\x1b[94m");
    }

    #[test]
    fn color_ansi_fg_rgb() {
        assert_eq!(Color::Rgb(255, 128, 0).ansi_fg(), "\x1b[38;2;255;128;0m");
        assert_eq!(Color::Rgb(0, 0, 0).ansi_fg(), "\x1b[38;2;0;0;0m");
    }

    #[test]
    fn color_ansi_bg_basic() {
        assert_eq!(Color::Red.ansi_bg(), "\x1b[41m");
        assert_eq!(Color::Green.ansi_bg(), "\x1b[42m");
        assert_eq!(Color::Blue.ansi_bg(), "\x1b[44m");
        assert_eq!(Color::Default.ansi_bg(), "\x1b[49m");
    }

    #[test]
    fn color_ansi_bg_bright() {
        assert_eq!(Color::BrightRed.ansi_bg(), "\x1b[101m");
        assert_eq!(Color::BrightGreen.ansi_bg(), "\x1b[102m");
    }

    #[test]
    fn color_ansi_bg_rgb() {
        assert_eq!(Color::Rgb(10, 20, 30).ansi_bg(), "\x1b[48;2;10;20;30m");
    }

    #[test]
    fn theme_dark_defaults() {
        let t = Theme::default_dark();
        assert_eq!(t.name, "dark");
        assert_eq!(t.header_color, Color::BrightBlue);
        assert_eq!(t.agent_idle, Color::White);
        assert_eq!(t.agent_busy, Color::BrightGreen);
        assert_eq!(t.agent_error, Color::BrightRed);
        assert_eq!(t.agent_dead, Color::Red);
        assert_eq!(t.task_pending, Color::Yellow);
        assert_eq!(t.task_active, Color::Cyan);
        assert_eq!(t.task_done, Color::Green);
        assert_eq!(t.task_failed, Color::Red);
        assert_eq!(t.border, Color::Blue);
        assert_eq!(t.prompt, Color::BrightGreen);
        assert_eq!(t.info, Color::Cyan);
        assert_eq!(t.warning, Color::BrightYellow);
        assert_eq!(t.error, Color::BrightRed);
    }

    #[test]
    fn theme_light_defaults() {
        let t = Theme::default_light();
        assert_eq!(t.name, "light");
        assert_eq!(t.header_color, Color::Blue);
        assert_eq!(t.agent_idle, Color::Default);
    }

    #[test]
    fn theme_minimal_defaults() {
        let t = Theme::minimal();
        assert_eq!(t.name, "minimal");
        assert_eq!(t.header_color, Color::White);
        assert_eq!(t.agent_idle, Color::Default);
        assert_eq!(t.task_pending, Color::Default);
    }

    #[test]
    fn theme_default_is_dark() {
        let t = Theme::default();
        assert_eq!(t.name, "dark");
    }

    #[test]
    fn theme_serialization_round_trip() {
        let theme = Theme::default_dark();
        let json = serde_json::to_string(&theme).unwrap();
        let back: Theme = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "dark");
        assert_eq!(back.header_color, Color::BrightBlue);
        assert_eq!(back.error, Color::BrightRed);
    }

    #[test]
    fn color_enum_serialization() {
        let c = Color::Rgb(100, 200, 50);
        let json = serde_json::to_string(&c).unwrap();
        let back: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn color_simple_serialization() {
        let c = Color::BrightYellow;
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("bright_yellow"));
        let back: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Color::BrightYellow);
    }

    #[test]
    fn all_colors_have_unique_fg_codes() {
        let colors = vec![
            Color::Default,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Cyan,
            Color::Magenta,
            Color::White,
            Color::BrightRed,
            Color::BrightGreen,
            Color::BrightYellow,
            Color::BrightBlue,
        ];
        let codes: Vec<String> = colors.iter().map(|c| c.ansi_fg()).collect();
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(
                    codes[i], codes[j],
                    "Colors {:?} and {:?} have same fg code",
                    colors[i], colors[j]
                );
            }
        }
    }

    #[test]
    fn all_colors_have_unique_bg_codes() {
        let colors = vec![
            Color::Default,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Cyan,
            Color::Magenta,
            Color::White,
            Color::BrightRed,
            Color::BrightGreen,
            Color::BrightYellow,
            Color::BrightBlue,
        ];
        let codes: Vec<String> = colors.iter().map(|c| c.ansi_bg()).collect();
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(
                    codes[i], codes[j],
                    "Colors {:?} and {:?} have same bg code",
                    colors[i], colors[j]
                );
            }
        }
    }

    #[test]
    fn rgb_boundary_values() {
        assert_eq!(
            Color::Rgb(0, 0, 0).ansi_fg(),
            "\x1b[38;2;0;0;0m"
        );
        assert_eq!(
            Color::Rgb(255, 255, 255).ansi_fg(),
            "\x1b[38;2;255;255;255m"
        );
        assert_eq!(
            Color::Rgb(255, 255, 255).ansi_bg(),
            "\x1b[48;2;255;255;255m"
        );
    }
}
