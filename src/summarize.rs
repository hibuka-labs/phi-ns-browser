//! `summarize()` — what the snapshot line says about this pack's objects
//! (sdk-assembly §8).
//!
//! Only pack-owned objects answer here; model-built values fall through to
//! the engine's generic summary. `b` is the one root worth describing: the
//! CDP session state is the expensive thing a cell inherits, so the snapshot
//! line carries where the tab is without spending another round-trip.

use phi_ns::NsObject;

/// Last-known page state. Refreshed by the navigation and tab calls — a few
/// clicks stale at worst, and free of CDP latency at snapshot time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageState {
    /// Active tab title.
    pub title: String,
    /// Active tab URL.
    pub url: String,
    /// How many tabs are open.
    pub tabs: usize,
}

impl PageState {
    /// Remember a navigation outcome.
    pub fn note_page(&mut self, title: String, url: String) {
        self.title = title;
        self.url = url;
    }

    /// Remember the open-tab count.
    pub fn note_tabs(&mut self, tabs: usize) {
        self.tabs = tabs;
    }
}

/// The summary the snapshot line shows for `b`.
pub fn summarize_b(state: &PageState) -> String {
    let where_ = if state.url.is_empty() {
        "no page yet".to_string()
    } else if state.title.trim().is_empty() {
        state.url.clone()
    } else {
        format!("{} {}", state.title.trim(), state.url)
    };
    format!("{where_}, {} tab(s)", state.tabs)
}

/// Pack-owned roots answer; everything else is `None` (generic summary).
pub fn summarize_obj(name: &str, obj: &NsObject, state: &PageState) -> Option<String> {
    match (name, obj.kind.as_str()) {
        ("b", "BrowserNs") => Some(summarize_b(state)),
        // `El` / `ImageHandle` carry their own `_phi_summary_`; let the
        // engine's generic path pick it up.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_names_the_page_and_the_tab_count() {
        let mut state = PageState::default();
        assert_eq!(summarize_b(&state), "no page yet, 0 tab(s)");

        state.note_page("Example Domain".into(), "https://example.com/".into());
        state.note_tabs(2);
        assert_eq!(
            summarize_b(&state),
            "Example Domain https://example.com/, 2 tab(s)"
        );
    }
}
