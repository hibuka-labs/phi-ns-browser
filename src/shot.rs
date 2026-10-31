//! Screenshot → [`ImageRef`] (sdk-assembly §4.2).
//!
//! Three rules live here: the bytes go to disk and the model gets a `Path`
//! reference (rule 2 — `Inline` must be requested explicitly), `alt` is
//! never empty (rule 3), and nothing is written to `stdout` by this layer —
//! the engine folds the event, the pack only produces it.

use crate::session::BrowserSession;
use phi_ns::ImageRef;
use std::path::PathBuf;
use std::sync::Mutex;

/// Where captures land and what they were last called, so the state summary
/// can answer without another CDP round-trip.
pub struct ShotStore {
    dir: PathBuf,
    counter: Mutex<u64>,
}

impl ShotStore {
    /// Screenshots land in `dir` (created on first capture).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            counter: Mutex::new(0),
        }
    }

    /// Capture the active viewport and hand back the model-side reference.
    ///
    /// `alt` describes what the model should look at; when the caller passes
    /// none, the page identity stands in — rule 3 is never violated.
    pub fn capture(
        &self,
        session: &mut BrowserSession,
        alt: Option<&str>,
    ) -> Result<ImageRef, String> {
        let bytes = session.screenshot()?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("cannot create shot dir {}: {e}", self.dir.display()))?;

        let n = {
            let mut counter = self.counter.lock().unwrap_or_else(|e| e.into_inner());
            *counter += 1;
            *counter
        };
        let path = self.dir.join(format!("shot-{n:04}.png"));
        std::fs::write(&path, &bytes)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;

        let alt = match alt.map(str::trim) {
            Some(a) if !a.is_empty() => a.to_string(),
            _ => {
                let (title, url) = session.page_identity()?;
                fallback_alt(&title, &url)
            }
        };
        Ok(ImageRef::from_path("image/png", path, alt))
    }
}

/// Page-derived stand-in when the caller gives no `alt`. Never empty —
/// rule 3 is what keeps the model from going blind on a trimmed event.
fn fallback_alt(title: &str, url: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        format!("Screenshot of {url}")
    } else {
        format!("Screenshot of {title} ({url})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_alt_is_never_empty() {
        let with_title = fallback_alt("Example Domain", "https://example.com/");
        assert!(with_title.contains("Example Domain"), "{with_title}");
        assert!(with_title.contains("example.com"), "{with_title}");

        let no_title = fallback_alt("   ", "https://example.com/");
        assert_eq!(no_title, "Screenshot of https://example.com/");
    }
}
