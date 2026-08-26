//! Single-line status renderer.

use linefeed_core::{Tracker, TrackerEvent};

const LINE_BUDGET: usize = 58;

/// `[   12.34s] TRACKING   42/317 ( 13.2%)  L 3 │ current line text`
pub fn render(tracker: &Tracker, ev: &TrackerEvent) -> String {
    let total = tracker.n_tokens();
    let line_no = tracker.current_line().map(|l| l + 1).unwrap_or(0);
    let text = truncate_chars(tracker.line_text().unwrap_or(""), LINE_BUDGET);
    format!(
        "[{t:>8.2}s] {state:<9} {cursor:>3}/{total} ({pct:>5.1}%)  L{line_no:>2} │ {text}",
        t = ev.t,
        state = ev.state.as_str(),
        cursor = ev.cursor,
        pct = tracker.percent(),
    )
}

pub fn final_summary(tracker: &Tracker) -> String {
    format!(
        "final: cursor {}/{} ({:.1}%) state {}, {} hypotheses, {} words",
        tracker.cursor(),
        tracker.n_tokens(),
        tracker.percent(),
        tracker.state().as_str(),
        tracker.n_hypotheses,
        tracker.n_words,
    )
}

/// Char-safe truncation with an ellipsis.
fn truncate_chars(s: &str, budget: usize) -> String {
    if s.chars().count() <= budget {
        return s.to_string();
    }
    let cut: String = s.chars().take(budget.saturating_sub(1)).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use linefeed_core::{Script, TrackerConfig};

    #[test]
    fn truncation_is_char_safe() {
        let long = "ação e reação — ".repeat(10);
        let t = truncate_chars(&long, 20);
        assert_eq!(t.chars().count(), 20);
        assert!(t.ends_with('…'));
        assert_eq!(truncate_chars("curto", 20), "curto");
    }

    #[test]
    fn final_summary_has_grep_anchor() {
        let tracker = Tracker::new(Script::parse("uma linha"), TrackerConfig::default());
        let s = final_summary(&tracker);
        assert!(s.starts_with("final: cursor"), "CI greps this prefix: {s}");
    }
}
