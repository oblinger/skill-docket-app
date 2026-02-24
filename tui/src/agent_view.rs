//! Agent detail view — displays captured tmux pane output for a single agent.
//!
//! Renders the agent's name as a bordered title and the captured terminal
//! output as scrollable text. Scroll offset allows the user to page through
//! long output histories.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};


/// Render the agent conversation view: captured pane output with scroll.
///
/// `agent_name` — the name shown in the border title.
/// `captured_output` — the raw text captured from the agent's tmux pane.
/// `scroll_offset` — vertical scroll position (0 = top).
pub fn render_agent_view(
    frame: &mut Frame,
    area: Rect,
    agent_name: &str,
    captured_output: &str,
    scroll_offset: u16,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Agent: {} ", agent_name));

    let paragraph = Paragraph::new(captured_output)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));

    frame.render_widget(paragraph, area);
}


/// Calculate the maximum scroll offset for a given content and viewport.
///
/// `line_count` — the total number of lines in the content.
/// `viewport_height` — the visible height (excluding borders).
///
/// Returns 0 if the content fits within the viewport.
pub fn max_scroll_offset(line_count: usize, viewport_height: u16) -> u16 {
    // Subtract 2 for top and bottom borders
    let usable = viewport_height.saturating_sub(2) as usize;
    if line_count > usable {
        (line_count - usable) as u16
    } else {
        0
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_scroll_content_fits() {
        // 5 lines of content, viewport of 10 (minus 2 borders = 8 usable)
        assert_eq!(max_scroll_offset(5, 10), 0);
    }

    #[test]
    fn max_scroll_content_exceeds() {
        // 20 lines, viewport 12 (minus 2 = 10 usable), so 10 lines of scroll
        assert_eq!(max_scroll_offset(20, 12), 10);
    }

    #[test]
    fn max_scroll_exact_fit() {
        // 8 lines, viewport 10 (minus 2 = 8 usable), exactly fits
        assert_eq!(max_scroll_offset(8, 10), 0);
    }

    #[test]
    fn max_scroll_tiny_viewport() {
        // viewport of 2 means 0 usable lines
        assert_eq!(max_scroll_offset(10, 2), 10);
    }

    #[test]
    fn max_scroll_zero_viewport() {
        assert_eq!(max_scroll_offset(10, 0), 10);
    }

    #[test]
    fn max_scroll_zero_lines() {
        assert_eq!(max_scroll_offset(0, 10), 0);
    }
}
