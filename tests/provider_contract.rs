//! Pack contract (sdk-assembly §9.7): the declaration source is the single
//! source, derived views cannot drift, and the browser face is exactly the
//! 24 methods the seed list advertises.

phi_ns::provider_contract_tests!(phi_ns_browser::PhiNsBrowser::new());

use phi_cap::Capability;
use phi_ns::NamespaceProvider;

#[test]
fn the_declaration_source_is_the_24_method_face() {
    let pack = phi_ns_browser::PhiNsBrowser::new();
    let methods = pack.methods();
    assert_eq!(methods.len(), 24, "the b.* face is 24 methods");

    for m in &methods {
        assert!(
            m.name.starts_with("b."),
            "method {} is not rooted at b",
            m.name
        );
        assert!(!m.doc.trim().is_empty(), "{} has no doc", m.name);
    }

    // Exactly one multimodal face — the screenshot.
    let multimodal: Vec<&str> = methods
        .iter()
        .filter(|m| m.multimodal)
        .map(|m| m.name)
        .collect();
    assert_eq!(
        multimodal,
        ["b.screenshot"],
        "only b.screenshot returns pictures"
    );

    // The seed list is literally one line per method (O1).
    let doc = pack.api_doc();
    assert_eq!(doc.lines().count(), 24, "one seed line per method:\n{doc}");
    for m in &methods {
        assert!(doc.contains(m.name), "seed list dropped {}", m.name);
    }
}

#[test]
fn the_capability_mapping_follows_the_action_face() {
    let methods = phi_ns_browser::PhiNsBrowser::new().methods();
    let class_of = |name: &str| {
        methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .capability()
    };

    assert_eq!(class_of("b.goto"), Capability::Net, "fetches a URL");
    assert_eq!(class_of("b.click"), Capability::Device, "input face");
    assert_eq!(class_of("b.type"), Capability::Device, "input face");
    assert_eq!(class_of("b.screenshot"), Capability::Device, "capture face");
    assert_eq!(class_of("b.text"), Capability::Device, "capture face");
    assert_eq!(class_of("b.wait"), Capability::Pure, "just sleeps");
    assert_eq!(
        class_of("b.restart"),
        Capability::Process,
        "process lifecycle"
    );
    assert_eq!(
        class_of("b.close"),
        Capability::Process,
        "process lifecycle"
    );

    // The wrinkle MethodPolicy exists for: same coarse class, opposite
    // posture. If this ever stops being true, re-read the O13-C board.
    assert_eq!(class_of("b.click"), class_of("b.screenshot"));
}

#[test]
fn the_snapshot_line_describes_b_only() {
    use phi_ns::NsObject;
    let pack = phi_ns_browser::PhiNsBrowser::new();
    let b = NsObject {
        name: "b".into(),
        kind: "BrowserNs".into(),
        repr: "BrowserNs()".into(),
        detail: None,
    };
    assert_eq!(
        pack.summarize("b", &b).as_deref(),
        Some("no page yet, 0 tab(s)")
    );

    let handle = NsObject {
        name: "shot".into(),
        kind: "ImageHandle".into(),
        repr: "ImageHandle('x')".into(),
        detail: Some("x".into()),
    };
    // Pack-owned roots answer; model-built values fall through to the engine.
    assert_eq!(pack.summarize("shot", &handle), None);
}
