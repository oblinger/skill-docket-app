//! Terminal rendering utilities -- ANSI formatting, table layout, box drawing.
//!
//! All functions produce `String` output. Nothing is written to stdout directly.
//! This module provides the building blocks that [`crate::status`] uses to
//! compose full status displays.

// ---------------------------------------------------------------------------
// ANSI escape constants
// ---------------------------------------------------------------------------

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const CYAN: &str = "\x1b[36m";
pub const WHITE: &str = "\x1b[37m";

// ---------------------------------------------------------------------------
// Box-drawing characters
// ---------------------------------------------------------------------------

pub const BOX_H: char = '\u{2500}';  // ─
pub const BOX_V: char = '\u{2502}';  // │
pub const BOX_TL: char = '\u{250C}'; // ┌
pub const BOX_TR: char = '\u{2510}'; // ┐
pub const BOX_BL: char = '\u{2514}'; // └
pub const BOX_BR: char = '\u{2518}'; // ┘
pub const BOX_T: char = '\u{252C}';  // ┬
pub const BOX_B: char = '\u{2534}';  // ┴
pub const BOX_L: char = '\u{251C}';  // ├
pub const BOX_R: char = '\u{2524}';  // ┤
pub const BOX_X: char = '\u{253C}';  // ┼

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

/// Truncate a string to `max_width` characters, appending an ellipsis if truncated.
/// If `max_width` < 3 the string is simply cut.
pub fn truncate(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_width {
        return s.to_string();
    }
    if max_width < 3 {
        return chars[..max_width].iter().collect();
    }
    let mut result: String = chars[..max_width - 1].iter().collect();
    result.push('\u{2026}'); // ellipsis character
    result
}

/// Pad a string on the right to exactly `width` characters.
/// If the string is longer, it is truncated.
pub fn pad_right(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= width {
        return truncate(s, width);
    }
    let mut result = s.to_string();
    for _ in 0..(width - chars.len()) {
        result.push(' ');
    }
    result
}

/// Pad a string on the left to exactly `width` characters.
/// If the string is longer, it is truncated.
pub fn pad_left(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= width {
        return truncate(s, width);
    }
    let padding = width - chars.len();
    let mut result = String::with_capacity(width);
    for _ in 0..padding {
        result.push(' ');
    }
    result.push_str(s);
    result
}

/// Center a string within `width` characters.
/// If the string is longer, it is truncated.
pub fn center(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= width {
        return truncate(s, width);
    }
    let total_padding = width - chars.len();
    let left_pad = total_padding / 2;
    let right_pad = total_padding - left_pad;
    let mut result = String::with_capacity(width);
    for _ in 0..left_pad {
        result.push(' ');
    }
    result.push_str(s);
    for _ in 0..right_pad {
        result.push(' ');
    }
    result
}

/// Strip ANSI escape sequences from a string for width calculation.
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        result.push(ch);
    }
    result
}

/// Visible width of a string (ignoring ANSI escape codes).
fn visible_width(s: &str) -> usize {
    strip_ansi(s).chars().count()
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

/// Column alignment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Alignment {
    Left,
    Right,
    Center,
}


/// Definition of a single table column.
#[derive(Debug, Clone)]
pub struct TableColumn {
    pub header: String,
    pub width: usize,
    pub align: Alignment,
}


/// A text-based table that renders to a `String`.
///
/// Supports optional box-drawing borders.
pub struct Table {
    columns: Vec<TableColumn>,
    rows: Vec<Vec<String>>,
    border: bool,
}


impl Table {
    /// Create a new table with the given column definitions.
    /// Borders are enabled by default.
    pub fn new(columns: Vec<TableColumn>) -> Self {
        Table {
            columns,
            rows: Vec::new(),
            border: true,
        }
    }

    /// Create a borderless table.
    pub fn borderless(columns: Vec<TableColumn>) -> Self {
        Table {
            columns,
            rows: Vec::new(),
            border: false,
        }
    }

    /// Add a row of cell values.
    pub fn add_row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    /// Render the table to a plain string (no ANSI colors on structure).
    pub fn render(&self) -> String {
        self.render_internal(false)
    }

    /// Render the table with ANSI color on borders and headers.
    pub fn render_with_color(&self) -> String {
        self.render_internal(true)
    }

    fn render_internal(&self, color: bool) -> String {
        let mut out = String::new();

        if self.border {
            // Top border
            out.push_str(&self.border_line(BOX_TL, BOX_T, BOX_TR, color));
            out.push('\n');
        }

        // Header row
        out.push_str(&self.render_row_cells(
            &self.columns.iter().map(|c| c.header.clone()).collect::<Vec<_>>(),
            color,
            true,
        ));
        out.push('\n');

        if self.border {
            // Header separator
            out.push_str(&self.border_line(BOX_L, BOX_X, BOX_R, color));
            out.push('\n');
        }

        // Data rows
        for row in &self.rows {
            out.push_str(&self.render_row_cells(row, color, false));
            out.push('\n');
        }

        if self.border {
            // Bottom border
            out.push_str(&self.border_line(BOX_BL, BOX_B, BOX_BR, color));
            out.push('\n');
        }

        out
    }

    fn border_line(&self, left: char, mid: char, right: char, color: bool) -> String {
        let mut line = String::new();
        if color {
            line.push_str(BLUE);
        }
        line.push(left);
        for (i, col) in self.columns.iter().enumerate() {
            for _ in 0..(col.width + 2) {
                line.push(BOX_H);
            }
            if i < self.columns.len() - 1 {
                line.push(mid);
            }
        }
        line.push(right);
        if color {
            line.push_str(RESET);
        }
        line
    }

    fn render_row_cells(&self, cells: &[String], color: bool, is_header: bool) -> String {
        let mut line = String::new();
        if self.border {
            if color {
                line.push_str(BLUE);
            }
            line.push(BOX_V);
            if color {
                line.push_str(RESET);
            }
        }

        for (i, col) in self.columns.iter().enumerate() {
            let cell = cells.get(i).map(|s| s.as_str()).unwrap_or("");
            let formatted = if is_header && color {
                format!("{}{}{}", BOLD, align_str(cell, col.width, col.align), RESET)
            } else {
                align_str(cell, col.width, col.align)
            };

            line.push(' ');
            line.push_str(&formatted);
            line.push(' ');

            if self.border && i < self.columns.len() - 1 {
                if color {
                    line.push_str(BLUE);
                }
                line.push(BOX_V);
                if color {
                    line.push_str(RESET);
                }
            }
        }

        if self.border {
            if color {
                line.push_str(BLUE);
            }
            line.push(BOX_V);
            if color {
                line.push_str(RESET);
            }
        }

        line
    }
}


/// Align a string within a given width according to the alignment.
/// For strings containing ANSI codes, alignment is based on visible width.
fn align_str(s: &str, width: usize, align: Alignment) -> String {
    let vis_len = visible_width(s);
    if vis_len >= width {
        return s.to_string();
    }
    let padding = width - vis_len;
    match align {
        Alignment::Left => {
            let mut r = s.to_string();
            for _ in 0..padding {
                r.push(' ');
            }
            r
        }
        Alignment::Right => {
            let mut r = String::new();
            for _ in 0..padding {
                r.push(' ');
            }
            r.push_str(s);
            r
        }
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            let mut r = String::new();
            for _ in 0..left {
                r.push(' ');
            }
            r.push_str(s);
            for _ in 0..right {
                r.push(' ');
            }
            r
        }
    }
}

// ---------------------------------------------------------------------------
// Progress bar
// ---------------------------------------------------------------------------

/// Render a progress bar of the given `width` filled to `fraction` (0.0 to 1.0).
///
/// Uses block characters for a smooth fill effect.
pub fn progress_bar(width: usize, fraction: f64) -> String {
    let fraction = fraction.clamp(0.0, 1.0);
    if width < 2 {
        return String::new();
    }
    // Inner width excludes the brackets
    let inner = width - 2;
    let filled_exact = fraction * inner as f64;
    let filled_full = filled_exact as usize;
    let remainder = filled_exact - filled_full as f64;

    // Block characters for sub-cell precision: ░ ▒ ▓ █
    let partial_chars = [' ', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2588}'];

    let mut bar = String::with_capacity(width + 10);
    bar.push('[');
    for _ in 0..filled_full {
        bar.push('\u{2588}'); // █
    }
    if filled_full < inner {
        let idx = (remainder * 4.0).round() as usize;
        let idx = idx.min(partial_chars.len() - 1);
        bar.push(partial_chars[idx]);
        for _ in (filled_full + 1)..inner {
            bar.push(' ');
        }
    }
    bar.push(']');
    bar
}

/// Return a colored status indicator symbol for the given status string.
pub fn status_indicator(status: &str) -> String {
    match status {
        "idle" => format!("{}{}  {}", WHITE, '\u{25CB}', RESET),    // ○ hollow circle
        "busy" => format!("{}{}  {}", GREEN, '\u{25CF}', RESET),    // ● filled circle
        "stalled" => format!("{}{}  {}", YELLOW, '\u{25C6}', RESET),// ◆ diamond
        "error" => format!("{}{}  {}", RED, '\u{2716}', RESET),     // ✖ cross
        "dead" => format!("{}{}{}", RED, '\u{2620}', RESET),         // ☠ skull
        "pending" => format!("{}{}{}", YELLOW, '\u{25CB}', RESET),   // ○ yellow hollow
        "in_progress" => format!("{}{}{}", CYAN, '\u{25B6}', RESET), // ▶ play
        "completed" => format!("{}{}{}", GREEN, '\u{2714}', RESET),  // ✔ check
        "failed" => format!("{}{}{}", RED, '\u{2718}', RESET),       // ✘ ballot x
        "paused" => format!("{}{}{}", YELLOW, '\u{2016}', RESET),    // ‖ pause
        "cancelled" => format!("{}{}{}", DIM, '\u{2013}', RESET),    // – en dash
        "healthy" => format!("{}{}{}", GREEN, '\u{2714}', RESET),    // ✔
        "degraded" => format!("{}{}{}", YELLOW, '\u{26A0}', RESET),  // ⚠
        "unhealthy" => format!("{}{}{}", RED, '\u{2716}', RESET),    // ✖
        "unknown" => format!("{}{}{}", DIM, '?', RESET),
        _ => format!("{}{}{}", DIM, '\u{00B7}', RESET),              // · middle dot
    }
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

/// A bordered panel with a title and content lines.
pub struct Panel {
    pub title: String,
    pub width: usize,
    pub lines: Vec<String>,
}


impl Panel {
    /// Create a new empty panel.
    pub fn new(title: &str, width: usize) -> Self {
        Panel {
            title: title.to_string(),
            width,
            lines: Vec::new(),
        }
    }

    /// Add a line of text.
    pub fn add_line(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }

    /// Add a key-value line formatted as "  key: value".
    pub fn add_kv(&mut self, key: &str, value: &str) {
        self.lines.push(format!("  {}: {}", key, value));
    }

    /// Render the panel to a string with box-drawing borders.
    pub fn render(&self) -> String {
        if self.width < 4 {
            return String::new();
        }
        let inner_width = self.width - 2; // account for left and right border chars
        let mut out = String::new();

        // Top border with title
        out.push(BOX_TL);
        if !self.title.is_empty() {
            let title_display = if self.title.len() > inner_width.saturating_sub(4) {
                truncate(&self.title, inner_width.saturating_sub(4))
            } else {
                self.title.clone()
            };
            out.push(BOX_H);
            out.push(' ');
            out.push_str(&title_display);
            out.push(' ');
            let used = title_display.chars().count() + 3; // BOX_H + space + space
            for _ in used..inner_width {
                out.push(BOX_H);
            }
        } else {
            for _ in 0..inner_width {
                out.push(BOX_H);
            }
        }
        out.push(BOX_TR);
        out.push('\n');

        // Content lines
        for line in &self.lines {
            out.push(BOX_V);
            let line_trunc = truncate(line, inner_width);
            out.push_str(&pad_right(&line_trunc, inner_width));
            out.push(BOX_V);
            out.push('\n');
        }

        // If no lines, render one empty line
        if self.lines.is_empty() {
            out.push(BOX_V);
            out.push_str(&pad_right("", inner_width));
            out.push(BOX_V);
            out.push('\n');
        }

        // Bottom border
        out.push(BOX_BL);
        for _ in 0..inner_width {
            out.push(BOX_H);
        }
        out.push(BOX_BR);
        out.push('\n');

        out
    }
}

// ---------------------------------------------------------------------------
// Sparkline
// ---------------------------------------------------------------------------

/// Render a sparkline from a series of values.
///
/// The sparkline uses Unicode block characters to show a compact inline chart.
/// Values are scaled to the range of the data.
pub fn sparkline(values: &[f64], width: usize) -> String {
    if values.is_empty() || width == 0 {
        return String::new();
    }

    let spark_chars = [
        '\u{2581}', // ▁
        '\u{2582}', // ▂
        '\u{2583}', // ▃
        '\u{2584}', // ▄
        '\u{2585}', // ▅
        '\u{2586}', // ▆
        '\u{2587}', // ▇
        '\u{2588}', // █
    ];

    // Sample or repeat values to fill width
    let data: Vec<f64> = if values.len() >= width {
        // Take the last `width` values
        values[values.len() - width..].to_vec()
    } else {
        // Use all values and pad left with the first value
        let mut d = vec![values[0]; width - values.len()];
        d.extend_from_slice(values);
        d
    };

    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;

    let mut out = String::with_capacity(width);
    for &v in &data {
        let normalized = if range == 0.0 {
            0.5
        } else {
            (v - min) / range
        };
        let idx = (normalized * 7.0).round() as usize;
        let idx = idx.min(7);
        out.push(spark_chars[idx]);
    }
    out
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- truncate ---

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_with_ellipsis() {
        let result = truncate("hello world", 8);
        assert_eq!(result.chars().count(), 8);
        assert!(result.ends_with('\u{2026}')); // ellipsis
    }

    #[test]
    fn truncate_very_small() {
        assert_eq!(truncate("hello", 2), "he");
        assert_eq!(truncate("hello", 1), "h");
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate("", 5), "");
    }

    // --- pad_right ---

    #[test]
    fn pad_right_short() {
        assert_eq!(pad_right("hi", 5), "hi   ");
    }

    #[test]
    fn pad_right_exact() {
        assert_eq!(pad_right("hello", 5), "hello");
    }

    #[test]
    fn pad_right_overflow() {
        let result = pad_right("hello world", 5);
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn pad_right_empty() {
        assert_eq!(pad_right("", 3), "   ");
    }

    // --- pad_left ---

    #[test]
    fn pad_left_short() {
        assert_eq!(pad_left("42", 5), "   42");
    }

    #[test]
    fn pad_left_exact() {
        assert_eq!(pad_left("hello", 5), "hello");
    }

    #[test]
    fn pad_left_overflow() {
        let result = pad_left("hello world", 5);
        assert_eq!(result.chars().count(), 5);
    }

    // --- center ---

    #[test]
    fn center_short() {
        assert_eq!(center("hi", 6), "  hi  ");
    }

    #[test]
    fn center_odd_padding() {
        assert_eq!(center("hi", 5), " hi  ");
    }

    #[test]
    fn center_exact() {
        assert_eq!(center("hello", 5), "hello");
    }

    #[test]
    fn center_overflow() {
        let result = center("hello world", 5);
        assert_eq!(result.chars().count(), 5);
    }

    // --- strip_ansi / visible_width ---

    #[test]
    fn strip_ansi_no_escapes() {
        assert_eq!(strip_ansi("hello"), "hello");
    }

    #[test]
    fn strip_ansi_with_color() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
    }

    #[test]
    fn visible_width_with_ansi() {
        let s = format!("{}hello{}", RED, RESET);
        assert_eq!(visible_width(&s), 5);
    }

    // --- Table ---

    #[test]
    fn table_basic_render() {
        let cols = vec![
            TableColumn { header: "Name".into(), width: 10, align: Alignment::Left },
            TableColumn { header: "Age".into(), width: 5, align: Alignment::Right },
        ];
        let mut table = Table::new(cols);
        table.add_row(vec!["Alice".into(), "30".into()]);
        table.add_row(vec!["Bob".into(), "25".into()]);
        let output = table.render();

        assert!(output.contains("Name"));
        assert!(output.contains("Age"));
        assert!(output.contains("Alice"));
        assert!(output.contains("Bob"));
        // Check borders are present
        assert!(output.contains(&BOX_TL.to_string()));
        assert!(output.contains(&BOX_BR.to_string()));
    }

    #[test]
    fn table_borderless_render() {
        let cols = vec![
            TableColumn { header: "X".into(), width: 5, align: Alignment::Left },
        ];
        let mut table = Table::borderless(cols);
        table.add_row(vec!["val".into()]);
        let output = table.render();

        // No border characters
        assert!(!output.contains(&BOX_TL.to_string()));
        assert!(!output.contains(&BOX_V.to_string()));
        assert!(output.contains("X"));
        assert!(output.contains("val"));
    }

    #[test]
    fn table_empty() {
        let cols = vec![
            TableColumn { header: "Col".into(), width: 8, align: Alignment::Left },
        ];
        let table = Table::new(cols);
        let output = table.render();

        // Should have header but no data rows
        assert!(output.contains("Col"));
        // Top, header-sep, and bottom borders
        let border_lines: Vec<&str> = output
            .lines()
            .filter(|l| l.starts_with(BOX_TL) || l.starts_with(BOX_L) || l.starts_with(BOX_BL))
            .collect();
        assert_eq!(border_lines.len(), 3);
    }

    #[test]
    fn table_alignment() {
        let cols = vec![
            TableColumn { header: "L".into(), width: 10, align: Alignment::Left },
            TableColumn { header: "R".into(), width: 10, align: Alignment::Right },
            TableColumn { header: "C".into(), width: 10, align: Alignment::Center },
        ];
        let mut table = Table::borderless(cols);
        table.add_row(vec!["ab".into(), "cd".into(), "ef".into()]);
        let output = table.render();

        let lines: Vec<&str> = output.lines().collect();
        // Data row (line index 1, since 0 is header)
        let data_line = lines[1];
        // Left-aligned "ab" should be near start
        assert!(data_line.contains("ab        "));
        // Right-aligned "cd" should have leading spaces
        assert!(data_line.contains("        cd"));
    }

    #[test]
    fn table_with_color_has_ansi() {
        let cols = vec![
            TableColumn { header: "X".into(), width: 5, align: Alignment::Left },
        ];
        let mut table = Table::new(cols);
        table.add_row(vec!["v".into()]);
        let output = table.render_with_color();

        // Should contain ANSI escape for blue borders
        assert!(output.contains("\x1b[34m"));
        // Should contain BOLD for header
        assert!(output.contains("\x1b[1m"));
    }

    #[test]
    fn table_missing_cells_handled() {
        let cols = vec![
            TableColumn { header: "A".into(), width: 5, align: Alignment::Left },
            TableColumn { header: "B".into(), width: 5, align: Alignment::Left },
        ];
        let mut table = Table::new(cols);
        // Only one cell for two columns
        table.add_row(vec!["x".into()]);
        let output = table.render();
        assert!(output.contains("x"));
    }

    // --- progress_bar ---

    #[test]
    fn progress_bar_empty() {
        let bar = progress_bar(20, 0.0);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
        // No filled blocks except possibly a space-like partial
        assert!(!bar.contains('\u{2588}')); // no full blocks
    }

    #[test]
    fn progress_bar_full() {
        let bar = progress_bar(12, 1.0);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
        // All inner chars should be full blocks
        let inner: String = bar.chars().skip(1).take(10).collect();
        for ch in inner.chars() {
            assert_eq!(ch, '\u{2588}');
        }
    }

    #[test]
    fn progress_bar_half() {
        let bar = progress_bar(22, 0.5);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
        assert_eq!(bar.chars().count(), 22);
    }

    #[test]
    fn progress_bar_clamps() {
        let bar_neg = progress_bar(10, -0.5);
        let bar_zero = progress_bar(10, 0.0);
        assert_eq!(bar_neg, bar_zero);

        let bar_over = progress_bar(10, 1.5);
        let bar_full = progress_bar(10, 1.0);
        assert_eq!(bar_over, bar_full);
    }

    #[test]
    fn progress_bar_too_small() {
        assert_eq!(progress_bar(1, 0.5), "");
        assert_eq!(progress_bar(0, 0.5), "");
    }

    // --- status_indicator ---

    #[test]
    fn status_indicator_known_statuses() {
        let statuses = [
            "idle", "busy", "stalled", "error", "dead",
            "pending", "in_progress", "completed", "failed",
            "paused", "cancelled", "healthy", "degraded",
            "unhealthy", "unknown",
        ];
        for s in &statuses {
            let ind = status_indicator(s);
            assert!(ind.contains('\x1b'), "status '{}' should contain ANSI codes", s);
            assert!(ind.contains(RESET), "status '{}' should end with RESET", s);
        }
    }

    #[test]
    fn status_indicator_unknown_gets_fallback() {
        let ind = status_indicator("bogus");
        assert!(ind.contains(RESET));
    }

    // --- Panel ---

    #[test]
    fn panel_basic_render() {
        let mut panel = Panel::new("Info", 30);
        panel.add_line("Hello world");
        panel.add_kv("Key", "Value");
        let output = panel.render();

        assert!(output.contains("Info"));
        assert!(output.contains("Hello world"));
        assert!(output.contains("Key: Value"));
        assert!(output.contains(&BOX_TL.to_string()));
        assert!(output.contains(&BOX_BR.to_string()));
    }

    #[test]
    fn panel_empty_content() {
        let panel = Panel::new("Empty", 20);
        let output = panel.render();

        assert!(output.contains("Empty"));
        // Should still have top, one empty line, and bottom
        let line_count = output.lines().count();
        assert_eq!(line_count, 3); // top, empty, bottom
    }

    #[test]
    fn panel_no_title() {
        let panel = Panel::new("", 20);
        let output = panel.render();
        let first_line = output.lines().next().unwrap();
        // Should be all horizontal lines between corners
        assert!(first_line.starts_with(BOX_TL));
        assert!(first_line.ends_with(BOX_TR));
    }

    #[test]
    fn panel_long_title_truncated() {
        let panel = Panel::new("This is a very long title that exceeds the width", 20);
        let output = panel.render();
        // Should render without panic and first line should fit in width
        let first_line = output.lines().next().unwrap();
        // The first line width might vary, but it shouldn't be way longer than panel width
        assert!(first_line.chars().count() <= 25); // some tolerance
    }

    #[test]
    fn panel_width_too_small() {
        let panel = Panel::new("X", 3);
        let output = panel.render();
        assert!(output.is_empty());
    }

    #[test]
    fn panel_kv_formatting() {
        let mut panel = Panel::new("Test", 40);
        panel.add_kv("agents", "3");
        panel.add_kv("tasks", "7");
        let output = panel.render();
        assert!(output.contains("  agents: 3"));
        assert!(output.contains("  tasks: 7"));
    }

    // --- sparkline ---

    #[test]
    fn sparkline_constant_values() {
        let sl = sparkline(&[5.0, 5.0, 5.0, 5.0], 4);
        assert_eq!(sl.chars().count(), 4);
        // All same value => all same character (mid-range block)
        let chars: Vec<char> = sl.chars().collect();
        assert!(chars.iter().all(|c| *c == chars[0]));
    }

    #[test]
    fn sparkline_ascending() {
        let sl = sparkline(&[0.0, 1.0, 2.0, 3.0], 4);
        assert_eq!(sl.chars().count(), 4);
        let chars: Vec<char> = sl.chars().collect();
        // Each char should be >= previous
        for i in 1..chars.len() {
            assert!(chars[i] >= chars[i - 1]);
        }
    }

    #[test]
    fn sparkline_descending() {
        let sl = sparkline(&[3.0, 2.0, 1.0, 0.0], 4);
        assert_eq!(sl.chars().count(), 4);
        let chars: Vec<char> = sl.chars().collect();
        // Each char should be <= previous
        for i in 1..chars.len() {
            assert!(chars[i] <= chars[i - 1]);
        }
    }

    #[test]
    fn sparkline_single_value() {
        let sl = sparkline(&[42.0], 5);
        assert_eq!(sl.chars().count(), 5);
    }

    #[test]
    fn sparkline_empty() {
        assert_eq!(sparkline(&[], 5), "");
    }

    #[test]
    fn sparkline_zero_width() {
        assert_eq!(sparkline(&[1.0, 2.0], 0), "");
    }

    #[test]
    fn sparkline_more_values_than_width() {
        // Should take the last `width` values
        let sl = sparkline(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], 3);
        assert_eq!(sl.chars().count(), 3);
    }

    #[test]
    fn sparkline_fewer_values_than_width() {
        // Should pad on left
        let sl = sparkline(&[1.0, 5.0], 6);
        assert_eq!(sl.chars().count(), 6);
    }

    // --- align_str ---

    #[test]
    fn align_str_left() {
        assert_eq!(align_str("hi", 6, Alignment::Left), "hi    ");
    }

    #[test]
    fn align_str_right() {
        assert_eq!(align_str("hi", 6, Alignment::Right), "    hi");
    }

    #[test]
    fn align_str_center() {
        assert_eq!(align_str("hi", 6, Alignment::Center), "  hi  ");
    }

    #[test]
    fn align_str_exact_width() {
        assert_eq!(align_str("hello", 5, Alignment::Left), "hello");
        assert_eq!(align_str("hello", 5, Alignment::Right), "hello");
        assert_eq!(align_str("hello", 5, Alignment::Center), "hello");
    }

    #[test]
    fn align_str_overflow() {
        // Longer string should be returned as-is (no truncation in align_str)
        assert_eq!(align_str("toolong", 3, Alignment::Left), "toolong");
    }

    #[test]
    fn align_str_ansi_width() {
        // String with ANSI codes: visible width is 2, but byte length is much more
        let s = format!("{}hi{}", RED, RESET);
        let result = align_str(&s, 6, Alignment::Left);
        // The visible content should be padded correctly
        assert_eq!(visible_width(&result), 6);
    }

    // --- box drawing constants ---

    #[test]
    fn box_chars_are_correct() {
        assert_eq!(BOX_H, '\u{2500}');
        assert_eq!(BOX_V, '\u{2502}');
        assert_eq!(BOX_TL, '\u{250C}');
        assert_eq!(BOX_TR, '\u{2510}');
        assert_eq!(BOX_BL, '\u{2514}');
        assert_eq!(BOX_BR, '\u{2518}');
        assert_eq!(BOX_T, '\u{252C}');
        assert_eq!(BOX_B, '\u{2534}');
        assert_eq!(BOX_L, '\u{251C}');
        assert_eq!(BOX_R, '\u{2524}');
        assert_eq!(BOX_X, '\u{253C}');
    }
}
