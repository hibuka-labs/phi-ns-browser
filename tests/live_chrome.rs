//! The pack driving a real page through its real host-call closures.
//!
//! No codexec here (packs must not know the engine — §9.3): a [`TestSink`]
//! stands at the worker seam, so what runs is exactly what `install()`
//! registers — `b.goto` … `b.screenshot` — against headless Chrome.

use async_trait::async_trait;
use phi_ns::{HostCall, NamespaceProvider, NsError, NsObject, WorkerSink};
use phi_ns_browser::PhiNsBrowser;
use std::collections::HashMap;
use std::sync::Arc;

/// The seam without the worker: registered fns by name, evals kept for the
/// shim assertion.
#[derive(Default)]
struct TestSink {
    fns: HashMap<String, Box<dyn HostCall>>,
    evals: Vec<String>,
}

impl TestSink {
    async fn call(
        &self,
        name: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, NsError> {
        self.fns
            .get(name)
            .unwrap_or_else(|| panic!("{name} was not registered"))
            .call(args)
            .await
    }
}

#[async_trait]
impl WorkerSink for TestSink {
    fn register_fn(&mut self, name: &str, call: Box<dyn HostCall>) -> Result<(), NsError> {
        assert!(
            name.starts_with("b."),
            "the pack only registers b.*: {name}"
        );
        self.fns.insert(name.to_string(), call);
        Ok(())
    }

    async fn eval(&mut self, code: &str) -> Result<(), NsError> {
        self.evals.push(code.to_string());
        Ok(())
    }

    async fn namespace_entries(&mut self) -> Result<Vec<NsObject>, NsError> {
        Ok(Vec::new())
    }
}

const FIXTURE: &str = r##"<!doctype html>
<html>
<head><title>Phi Pack Fixture</title></head>
<body>
  <div class="toolbar"><button id="export-btn">导出</button></div>
  <input id="q" />
  <p id="out">ready</p>
  <a id="next" href="#second">next section</a>
  <script>
    document.getElementById('export-btn').addEventListener('click', function () {
      document.getElementById('out').textContent = 'exported';
    });
  </script>
</body>
</html>
"##;

/// One pack, one sink, one fixture page — the whole face exercised in order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_b_face_drives_a_real_page() {
    let dir = tempfile::tempdir().expect("tempdir");
    let page = dir.path().join("fixture.html");
    std::fs::write(&page, FIXTURE).expect("fixture writes");
    let url = format!("file://{}", page.display());
    let shots = dir.path().join("shots");

    let pack = Arc::new(PhiNsBrowser::new().with_shot_dir(&shots));
    let mut sink = TestSink::default();
    pack.install(&mut sink).await.expect("install");

    // The shim is what makes `b` exist in the worker language.
    assert!(
        sink.evals
            .iter()
            .any(|e| e.contains("class BrowserNs") && e.contains("b = BrowserNs()")),
        "install() must eval the Python shim"
    );
    assert_eq!(sink.fns.len(), 24, "every declared method gets a host fn");

    // goto — first use spawns Chrome (lazy session).
    sink.call("b.goto", vec![serde_json::json!(url)])
        .await
        .expect("goto");

    // query(sel) → ElementHit; the selector prefers the id (buildSelector).
    let hits = sink
        .call("b.query", vec![serde_json::json!("button")])
        .await
        .expect("query button");
    let hits = hits.as_array().expect("hit list");
    assert_eq!(hits.len(), 1, "one button: {hits:?}");
    let sel = hits[0]["selector"].as_str().expect("selector").to_string();
    assert_eq!(sel, "#export-btn", "selector prefers the id: {sel}");
    assert_eq!(hits[0]["text"], "导出", "CJK text survives extraction");

    // query(None) → the interactive face.
    let interactive = sink
        .call("b.query", vec![serde_json::Value::Null])
        .await
        .expect("query interactive");
    let interactive = interactive.as_array().expect("hit list");
    assert!(
        interactive.len() >= 2,
        "button + input are interactive: {interactive:?}"
    );

    // click → the click handler runs.
    sink.call("b.click", vec![serde_json::json!(sel)])
        .await
        .expect("click");
    let out = sink
        .call(
            "b.eval",
            vec![serde_json::json!(
                "document.getElementById('out').textContent"
            )],
        )
        .await
        .expect("eval");
    assert_eq!(out, "exported", "the click handler ran");

    // type → the input fills.
    sink.call(
        "b.type",
        vec![serde_json::json!("#q"), serde_json::json!("hello")],
    )
    .await
    .expect("type");
    let value = sink
        .call(
            "b.eval",
            vec![serde_json::json!("document.getElementById('q').value")],
        )
        .await
        .expect("eval");
    assert_eq!(value, "hello", "the input filled");

    // text/markdown/html/links — the read face.
    let text = sink.call("b.text", vec![]).await.expect("text");
    let text = text.as_str().expect("text is a string");
    assert!(
        text.contains("导出") && text.contains("exported"),
        "page text: {text}"
    );

    let md = sink.call("b.markdown", vec![]).await.expect("markdown");
    assert!(md.as_str().expect("md").contains("next section"), "{md}");

    let html = sink.call("b.html", vec![]).await.expect("html");
    assert!(
        html.as_str().expect("html").contains("export-btn"),
        "{html}"
    );

    let links = sink.call("b.links", vec![]).await.expect("links");
    assert!(
        links.as_str().expect("links").contains("next section"),
        "{links}"
    );

    // screenshot(alt) → the wire value the engine harvests.
    let wire = sink
        .call(
            "b.screenshot",
            vec![serde_json::json!("the export button row")],
        )
        .await
        .expect("screenshot");
    assert_eq!(wire["mime"], "image/png");
    assert_eq!(wire["alt"], "the export button row");
    let path = std::path::PathBuf::from(wire["path"].as_str().expect("path"));
    let bytes = std::fs::read(&path).expect("shot bytes");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "a real PNG on disk");
    assert!(
        path.starts_with(&shots),
        "shot lands in the store: {path:?}"
    );

    // screenshot() with no alt → page-derived fallback, never empty (rule 3).
    let wire = sink
        .call("b.screenshot", vec![serde_json::Value::Null])
        .await
        .expect("screenshot fallback");
    let alt = wire["alt"].as_str().expect("alt");
    assert!(
        alt.contains("Phi Pack Fixture"),
        "alt comes from the page: {alt}"
    );
    assert!(alt.contains("fixture.html"), "alt carries the url: {alt}");

    // tabs() lists the open page.
    let tabs = sink.call("b.tabs", vec![]).await.expect("tabs");
    let tabs = tabs.as_str().expect("tabs string");
    assert!(tabs.contains("Phi Pack Fixture"), "tab list: {tabs}");

    // The snapshot summary follows the navigation.
    let root = NsObject {
        name: "b".into(),
        kind: "BrowserNs".into(),
        repr: "BrowserNs()".into(),
        detail: None,
    };
    let summary = pack.summarize("b", &root).expect("b summarizes itself");
    assert!(
        summary.contains("Phi Pack Fixture") && summary.contains("fixture.html"),
        "summary names the page: {summary}"
    );
    // Count is honest but not over-pinned: spawn keeps its initial tab plus
    // the one it opens on purpose (ported behavior).
    assert!(
        summary.contains("tab(s)") && !summary.contains("0 tab"),
        "summary counts open tabs: {summary}"
    );

    // close() is the lifecycle face; on_session_end walks the same door.
    sink.call("b.close", vec![]).await.expect("close");
    pack.on_session_end().await.expect("session end");
}
