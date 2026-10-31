//! # phi-ns-browser — the `b.*` capability pack
//!
//! Interface migration over the moved browser core (sdk-assembly §7.1): the
//! CDP session and DOM extraction come from phi-tools' stack, the 22 tool
//! shells do not. What the model writes is code against `b.*` (§4.2 sample):
//!
//! ```python
//! b.goto(url)
//! for el in b.query(".toolbar button"):
//!     if "导出" in el.text:
//!         el.click()
//!         break
//! shot = b.screenshot()              # -> ImageHandle;also emits CellEvent::Image
//! ```
//!
//! The pack speaks only the `phi-ns` contract — assembly (`.with_ns`) and
//! policy live above it (§9.3: `phi-ns-* ↛ codexec`).

pub mod dom;
pub mod methods;
pub mod session;
pub mod shot;
pub mod summarize;

use crate::session::{BrowserSession, LaunchOptions};
use crate::shot::ShotStore;
use crate::summarize::{PageState, summarize_obj};
use async_trait::async_trait;
use phi_ns::{HostCall, MethodSpec, NamespaceProvider, NsError, NsObject, WorkerSink};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Worker-side face of the pack: `b`, `El`, `ImageHandle`.
///
/// Every method is one line of glue over `_phi_call`, so policy and journal
/// see exactly the declared names — no side door.
const PY_SHIM: &str = r#"
class ImageHandle:
    """A picture the code can pass around and the model can look at."""
    def __init__(self, d):
        self.path = d.get("path")
        self.mime = d.get("mime", "image/png")
        self.alt = d.get("alt", "")

    def save(self, dst):
        import shutil
        shutil.copyfile(self.path, dst)
        return dst

    def __repr__(self):
        return "ImageHandle(%r)" % self.alt

    def _phi_summary_(self):
        return self.alt


class El:
    """One element handle from b.query: .text/.tag plus the actions."""
    def __init__(self, d):
        self._sel = d.get("selector", "")
        self.text = d.get("text", "")
        self.tag = d.get("tag", "")

    def click(self):
        return _phi_call("b.click", [self._sel])

    def type(self, text):
        return _phi_call("b.type", [self._sel, text])

    def hover(self):
        return _phi_call("b.hover", [self._sel])

    def select(self, value):
        return _phi_call("b.select", [self._sel, value])

    def __repr__(self):
        return "El(%s, %r)" % (self.tag, self.text)

    def _phi_summary_(self):
        return "%s %r" % (self.tag, self.text)


class BrowserNs:
    def goto(self, url):
        return _phi_call("b.goto", [url])

    def back(self):
        return _phi_call("b.back", [])

    def forward(self):
        return _phi_call("b.forward", [])

    def query(self, sel=None):
        return [El(h) for h in _phi_call("b.query", [sel])]

    def click(self, sel):
        return _phi_call("b.click", [sel])

    def type(self, sel, text):
        return _phi_call("b.type", [sel, text])

    def hover(self, sel):
        return _phi_call("b.hover", [sel])

    def select(self, sel, value):
        return _phi_call("b.select", [sel, value])

    def press(self, key):
        return _phi_call("b.press", [key])

    def scroll(self, direction, amount=400):
        return _phi_call("b.scroll", [direction, amount])

    def eval(self, js):
        return _phi_call("b.eval", [js])

    def text(self):
        return _phi_call("b.text", [])

    def markdown(self):
        return _phi_call("b.markdown", [])

    def html(self):
        return _phi_call("b.html", [])

    def links(self):
        return _phi_call("b.links", [])

    def snapshot(self):
        return _phi_call("b.snapshot", [])

    def screenshot(self, alt=None):
        return ImageHandle(_phi_call("b.screenshot", [alt]))

    def tabs(self):
        return _phi_call("b.tabs", [])

    def new_tab(self, url=None):
        return _phi_call("b.new_tab", [url])

    def switch_tab(self, index):
        return _phi_call("b.switch_tab", [index])

    def close_tab(self, index):
        return _phi_call("b.close_tab", [index])

    def wait(self, seconds=1.0):
        return _phi_call("b.wait", [seconds])

    def restart(self):
        return _phi_call("b.restart", [])

    def close(self):
        return _phi_call("b.close", [])


b = BrowserNs()
"#;

/// One host-side implementation of a `b.*` method.
type SessionFn = dyn Fn(
        &mut BrowserSession,
        &mut PageState,
        &ShotStore,
        &[serde_json::Value],
    ) -> Result<serde_json::Value, String>
    + Send
    + Sync;

/// The `b.*` capability pack: one lazily-started CDP session behind the
/// declared methods.
pub struct PhiNsBrowser {
    session: Arc<Mutex<BrowserSession>>,
    state: Arc<Mutex<PageState>>,
    shots: Arc<ShotStore>,
}

impl PhiNsBrowser {
    /// Default pack: headless Chrome on first use, screenshots under the
    /// process temp directory.
    pub fn new() -> Self {
        Self::with_launch(LaunchOptions::default())
    }

    /// Pack with explicit launch options (window size, binary path, …).
    pub fn with_launch(launch: LaunchOptions) -> Self {
        let shot_dir = std::env::temp_dir().join(format!("phi-ns-browser-{}", std::process::id()));
        Self {
            session: Arc::new(Mutex::new(BrowserSession::lazy(launch))),
            state: Arc::new(Mutex::new(PageState::default())),
            shots: Arc::new(ShotStore::new(shot_dir)),
        }
    }

    /// Where screenshots land.
    pub fn with_shot_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.shots = Arc::new(ShotStore::new(dir));
        self
    }

    /// The method implementations, one per declared row in [`methods`].
    fn calls(&self) -> Vec<(&'static str, Arc<SessionFn>)> {
        fn one<F>(f: F) -> Arc<SessionFn>
        where
            F: Fn(
                    &mut BrowserSession,
                    &mut PageState,
                    &ShotStore,
                    &[serde_json::Value],
                ) -> Result<serde_json::Value, String>
                + Send
                + Sync
                + 'static,
        {
            Arc::new(f)
        }

        vec![
            (
                "b.goto",
                one(|session, state, _shots, args| {
                    let url = str_arg(args, 0)?;
                    session.navigate(&url)?;
                    let _ = session.wait_for_navigation();
                    refresh(session, state);
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.back",
                one(|session, state, _shots, _args| {
                    session.go_back()?;
                    refresh(session, state);
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.forward",
                one(|session, state, _shots, _args| {
                    session.go_forward()?;
                    refresh(session, state);
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.query",
                one(|session, _state, _shots, args| {
                    let hits = match opt_str_arg(args, 0)? {
                        Some(css) => {
                            let tab = session.tab()?;
                            dom::query_by_selector(&tab, &css)?
                        }
                        None => {
                            let tab = session.tab()?;
                            dom::query_interactive(&tab)?
                        }
                    };
                    Ok(serde_json::Value::Array(
                        hits.into_iter()
                            .map(|h| {
                                serde_json::json!({
                                    "selector": h.selector,
                                    "text": h.text,
                                    "tag": h.tag,
                                })
                            })
                            .collect(),
                    ))
                }),
            ),
            (
                "b.click",
                one(|session, _state, _shots, args| {
                    let sel = str_arg(args, 0)?;
                    session.click_element(&sel)?;
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.type",
                one(|session, _state, _shots, args| {
                    let sel = str_arg(args, 0)?;
                    let text = str_arg(args, 1)?;
                    session.type_text(&sel, &text)?;
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.hover",
                one(|session, _state, _shots, args| {
                    let sel = str_arg(args, 0)?;
                    session.hover_element(&sel)?;
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.select",
                one(|session, _state, _shots, args| {
                    let sel = str_arg(args, 0)?;
                    let value = str_arg(args, 1)?;
                    session.select_option(&sel, &value)?;
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.press",
                one(|session, _state, _shots, args| {
                    let key = str_arg(args, 0)?;
                    session.press_key(&key)?;
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.scroll",
                one(|session, _state, _shots, args| {
                    let direction = str_arg(args, 0)?;
                    let amount = num_arg(args, 1, 400.0).map(|v| v as u32)?;
                    session.scroll(&direction, amount)?;
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.eval",
                one(|session, _state, _shots, args| {
                    let js = str_arg(args, 0)?;
                    // CDP evaluate hands back primitives only; JSON-round-trip the
                    // completion value so the declared face — "its JSON-serializable
                    // result" — is true for objects and arrays too.
                    let wrapped = format!(
                        "JSON.stringify((0, eval)({}))",
                        serde_json::to_string(&js).map_err(|e| e.to_string())?
                    );
                    let value = session.evaluate(&wrapped)?;
                    match value.as_str() {
                        Some(text) => serde_json::from_str(text)
                            .map_err(|e| format!("eval result is not JSON: {e}")),
                        // undefined / non-serializable completion.
                        None => Ok(serde_json::Value::Null),
                    }
                }),
            ),
            (
                "b.text",
                one(|session, _state, _shots, _args| {
                    let value = session.evaluate("document.body ? document.body.innerText : ''")?;
                    Ok(serde_json::json!(value.as_str().unwrap_or_default()))
                }),
            ),
            (
                "b.markdown",
                one(|session, _state, _shots, _args| {
                    Ok(serde_json::json!(session.get_markdown()?))
                }),
            ),
            (
                "b.html",
                one(|session, _state, _shots, _args| Ok(serde_json::json!(session.get_html()?))),
            ),
            (
                "b.links",
                one(|session, _state, _shots, _args| Ok(serde_json::json!(session.read_links()?))),
            ),
            (
                "b.snapshot",
                one(|session, _state, _shots, _args| {
                    let tab = session.tab()?;
                    Ok(serde_json::json!(dom::snapshot_text(&tab)?))
                }),
            ),
            (
                "b.screenshot",
                one(|session, _state, shots, args| {
                    let alt = opt_str_arg(args, 0)?;
                    let image = shots.capture(session, alt.as_deref())?;
                    // One source, two channels: the wire value becomes the
                    // worker's ImageHandle and the engine's CellEvent::Image.
                    Ok(image.to_wire())
                }),
            ),
            (
                "b.tabs",
                one(|session, _state, _shots, _args| {
                    let tabs = session.get_tabs()?;
                    let mut out = String::new();
                    for (i, tab) in tabs.iter().enumerate() {
                        out.push_str(&format!(
                            "[{}] {} {}\n",
                            i,
                            tab.get_title().unwrap_or_default(),
                            tab.get_url()
                        ));
                    }
                    Ok(serde_json::json!(out))
                }),
            ),
            (
                "b.new_tab",
                one(|session, state, _shots, args| {
                    session.new_tab()?;
                    if let Some(url) = opt_str_arg(args, 0)? {
                        session.navigate(&url)?;
                        let _ = session.wait_for_navigation();
                    }
                    refresh(session, state);
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.switch_tab",
                one(|session, state, _shots, args| {
                    let index = usize_arg(args, 0, 0)?;
                    let tabs = session.get_tabs()?;
                    let tab = tabs.get(index).ok_or_else(|| {
                        format!("tab index {index} out of range ({} tabs)", tabs.len())
                    })?;
                    tab.activate()
                        .map_err(|e| format!("activate failed: {e}"))?;
                    refresh(session, state);
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.close_tab",
                one(|session, state, _shots, args| {
                    let index = usize_arg(args, 0, 0)?;
                    let tabs = session.get_tabs()?;
                    let tab = tabs.get(index).ok_or_else(|| {
                        format!("tab index {index} out of range ({} tabs)", tabs.len())
                    })?;
                    tab.close(false).map_err(|e| format!("close failed: {e}"))?;
                    refresh(session, state);
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.wait",
                one(|_session, _state, _shots, args| {
                    let seconds = num_arg(args, 0, 1.0)?;
                    std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0)));
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.restart",
                one(|session, state, _shots, _args| {
                    session.restart()?;
                    refresh(session, state);
                    Ok(serde_json::Value::Null)
                }),
            ),
            (
                "b.close",
                one(|session, state, _shots, _args| {
                    session.close()?;
                    *state = PageState::default();
                    Ok(serde_json::Value::Null)
                }),
            ),
        ]
    }
}

impl Default for PhiNsBrowser {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NamespaceProvider for PhiNsBrowser {
    fn namespaces(&self) -> &'static [&'static str] {
        &["b"]
    }

    fn methods(&self) -> Vec<MethodSpec> {
        methods::method_table()
    }

    async fn install(&self, sink: &mut dyn WorkerSink) -> Result<(), NsError> {
        for (name, func) in self.calls() {
            sink.register_fn(
                name,
                Box::new(SessionCall {
                    session: Arc::clone(&self.session),
                    state: Arc::clone(&self.state),
                    shots: Arc::clone(&self.shots),
                    func,
                }),
            )?;
        }
        sink.eval(PY_SHIM).await
    }

    fn summarize(&self, name: &str, obj: &NsObject) -> Option<String> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        summarize_obj(name, obj, &state)
    }

    async fn on_session_end(&self) -> Result<(), NsError> {
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        session.close().map_err(NsError::Sink)?;
        Ok(())
    }
}

// ---- host-call plumbing -------------------------------------------------

struct SessionCall {
    session: Arc<Mutex<BrowserSession>>,
    state: Arc<Mutex<PageState>>,
    shots: Arc<ShotStore>,
    func: Arc<SessionFn>,
}

#[async_trait]
impl HostCall for SessionCall {
    async fn call(&self, args: Vec<serde_json::Value>) -> Result<serde_json::Value, NsError> {
        let session = Arc::clone(&self.session);
        let state = Arc::clone(&self.state);
        let shots = Arc::clone(&self.shots);
        let func = Arc::clone(&self.func);
        // headless_chrome is synchronous; the worker seam is not. Blocking
        // work stays off the async runtime (same shape the 22 shells used).
        let joined = tokio::task::spawn_blocking(move || {
            let mut session = session.lock().unwrap_or_else(|e| e.into_inner());
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            (*func)(&mut session, &mut state, &shots, &args)
        })
        .await;
        match joined {
            Err(e) => Err(NsError::Sink(format!("browser call panicked: {e}"))),
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(NsError::Sink(e)),
        }
    }
}

// ---- argument helpers (the shim speaks JSON, so does the seam) ----------

fn str_arg(args: &[serde_json::Value], i: usize) -> Result<String, String> {
    args.get(i)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("argument {} must be a string", i + 1))
}

fn opt_str_arg(args: &[serde_json::Value], i: usize) -> Result<Option<String>, String> {
    match args.get(i) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => v
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| format!("argument {} must be a string or None", i + 1)),
    }
}

fn num_arg(args: &[serde_json::Value], i: usize, default: f64) -> Result<f64, String> {
    match args.get(i) {
        None | Some(serde_json::Value::Null) => Ok(default),
        Some(v) => v
            .as_f64()
            .ok_or_else(|| format!("argument {} must be a number", i + 1)),
    }
}

fn usize_arg(args: &[serde_json::Value], i: usize, default: usize) -> Result<usize, String> {
    let n = num_arg(args, i, default as f64)?;
    if n < 0.0 {
        return Err(format!("argument {} must not be negative", i + 1));
    }
    Ok(n as usize)
}

/// Remember where the session is after a navigation/tab change, so the
/// snapshot line can answer without another CDP round-trip.
fn refresh(session: &mut BrowserSession, state: &mut PageState) {
    if let Ok((title, url)) = session.page_identity() {
        state.note_page(title, url);
    }
    if let Ok(tabs) = session.get_tabs() {
        state.note_tabs(tabs.len());
    }
}
