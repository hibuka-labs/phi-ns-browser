//! `b.*` MethodSpec table — **the single declaration source** (§3.1).
//!
//! Every view of the pack (seed list, capability review, policy keys) is
//! derived from these rows; nothing else declares anything.
//!
//! Capability mapping follows the action face, not the data meaning (§4.3):
//!
//! * `b.goto` fetches a URL → `net:http`
//! * actions that change what the tab does → `device:input{targets:[tab]}`
//! * reads of the tab → `device:capture{targets:[tab]}`
//! * browser process lifecycle (`restart`/`close`) → `process:spawn`
//!
//! One honest wrinkle: Chrome itself is spawned lazily, on whichever call
//! first needs it (see `session.rs`), so a policy that wants to gate the
//! spawn gates `b.restart`/`b.close` or pre-warms the session — the per-call
//! declarations name what each call *does*, which is what `class_of` keys on.
//!
//! Signatures live in the `doc` line because the seed list is the model's
//! only reference — `api_line()` renders `b.type — type(sel, text): …`.

use phi_cap::{device_capture, device_input, net_http, process_spawn_programs};
use phi_ns::{MethodSpec, m};

/// The whole `b.*` face, in seed-list order.
pub fn method_table() -> Vec<MethodSpec> {
    vec![
        m(
            "b.goto",
            &[net_http(&["*"])],
            "goto(url): navigate the active tab to a URL and wait for the load",
            false,
        ),
        m(
            "b.back",
            &[device_input(&["tab"])],
            "back(): step the active tab back in history",
            false,
        ),
        m(
            "b.forward",
            &[device_input(&["tab"])],
            "forward(): step the active tab forward in history",
            false,
        ),
        m(
            "b.query",
            &[device_capture(&["tab"])],
            "query(sel=None): find elements by CSS selector, or the page's interactive elements when sel is None; returns El list (.text/.tag/.click()/.type()/.hover()/.select())",
            false,
        ),
        m(
            "b.click",
            &[device_input(&["tab"])],
            "click(sel): click the element a selector points at (selectors come from b.query)",
            false,
        ),
        m(
            "b.type",
            &[device_input(&["tab"])],
            "type(sel, text): fill the element a selector points at with text",
            false,
        ),
        m(
            "b.hover",
            &[device_input(&["tab"])],
            "hover(sel): hover over the element a selector points at",
            false,
        ),
        m(
            "b.select",
            &[device_input(&["tab"])],
            "select(sel, value): choose a dropdown option value on the element a selector points at",
            false,
        ),
        m(
            "b.press",
            &[device_input(&["tab"])],
            "press(key): press a keyboard key on the page, e.g. \"Enter\"",
            false,
        ),
        m(
            "b.scroll",
            &[device_input(&["tab"])],
            "scroll(direction, amount=400): scroll the page \"up\"/\"down\"/\"left\"/\"right\" by amount pixels",
            false,
        ),
        m(
            "b.eval",
            &[device_input(&["tab"])],
            "eval(js): run JavaScript in the page and return its JSON-serializable result",
            false,
        ),
        m(
            "b.text",
            &[device_capture(&["tab"])],
            "text(): read the page as plain text",
            false,
        ),
        m(
            "b.markdown",
            &[device_capture(&["tab"])],
            "markdown(): read the page as Markdown",
            false,
        ),
        m(
            "b.html",
            &[device_capture(&["tab"])],
            "html(): read the page's HTML source",
            false,
        ),
        m(
            "b.links",
            &[device_capture(&["tab"])],
            "links(): read page links as [n] text -> href lines",
            false,
        ),
        m(
            "b.snapshot",
            &[device_capture(&["tab"])],
            "snapshot(): read the page's accessibility tree (roles, names, indices)",
            false,
        ),
        m(
            "b.screenshot",
            &[device_capture(&["tab"])],
            "screenshot(alt=None): capture the viewport as an ImageHandle (path/mime/alt); alt describes what to look at",
            true,
        ),
        m(
            "b.tabs",
            &[device_capture(&["tab"])],
            "tabs(): list open tabs as [index] title url lines",
            false,
        ),
        m(
            "b.new_tab",
            &[device_input(&["tab"])],
            "new_tab(url=None): open a new tab, optionally at a URL",
            false,
        ),
        m(
            "b.switch_tab",
            &[device_input(&["tab"])],
            "switch_tab(index): make the tab at index active",
            false,
        ),
        m(
            "b.close_tab",
            &[device_input(&["tab"])],
            "close_tab(index): close the tab at index",
            false,
        ),
        m(
            "b.wait",
            &[],
            "wait(seconds=1.0): sleep for seconds and let the page settle",
            false,
        ),
        m(
            "b.restart",
            &[process_spawn_programs(&["chrome"])],
            "restart(): restart the browser instance from scratch",
            false,
        ),
        m(
            "b.close",
            &[process_spawn_programs(&["chrome"])],
            "close(): close the browser; later calls fail until b.restart",
            false,
        ),
    ]
}
