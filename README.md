The `b.*` capability pack: browser automation as a namespace.

What the model writes is code against `b.*`:

```python
b.goto(url)
for el in b.query(".toolbar button"):
    if "Export" in el.text:
        el.click()
        break
shot = b.screenshot()          # -> ImageHandle; also emits CellEvent::Image
```

The pack speaks only the `phi-ns` contract — assembly and policy live above
it. It never depends on `codexec`.

```toml
[dependencies]
phi-ns-browser = "0.1.0"
```

## License

MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
