# Native clipboard drag probe

The probe creates its own PNG and UTF-8 file under a unique temporary directory.
It never reads or writes the system clipboard, clipboard history, pairing state,
or application settings. Its HTTP fixture listens only on `127.0.0.1`.

Run a debug example, without producing an installer or release package:

```sh
cd arcrelay-desktop
cargo run --example clipboard_drag_probe
```

For an isolated destination in the same window, using the real WebKit engine:

```sh
cargo run --example clipboard_drag_probe -- --embedded-webkit
```

Open the printed `BROWSER_URL` in a separate browser. Arrange its drop area beside
the probe window. Physically drag each source button into the drop area. The
source uses the production `src/clipboard_drag/native.rs` and AppKit adapter. It
starts only after AppKit receives a mouse-down followed by a mouse-drag event.
The HTML fixture does not synthesize drag events.

Check PNG, file, two files, Unicode text and HTML. Each successful drop must show
`trusted: true` and `pass: true`. File checks compare name, MIME type, size and
SHA-256 with `expected.json`. The default 3-second delayed read happens after the
native source receives its completion callback. Increase the delay for longer
lease checks. JSON reports also appear in the terminal and `reports.jsonl` in
the printed `FIXTURES` directory.

For HTML drops, the report records the offered `text/html` MIME type and compares
the complete HTML string with the generated fixture, including its `<strong>`
markup. It displays and saves that string as data without parsing or rendering
it. Browser-added document wrappers or equivalent HTML reserialization fail
this exact check; inspect the raw report and use the separate manual
contenteditable check below to verify rendered formatting in that browser.

The browser regression test exercises this reporting boundary with synthetic
DOM drops, including script and embedded-resource payloads. It verifies literal
reporting without execution or resource requests, not native drag delivery:

```sh
npx playwright test --config playwright.visual.config.ts clipboard-drag-probe.spec.ts
```

Additional manual cases:

- Drag a source, keep the mouse button held, press Escape, then release. Expect
  one `NATIVE_END ... Cancelled` and no browser report.
- Release outside a drop destination. Expect cancellation, not `Dropped`.
- Drag plain text into the ordinary textarea, and HTML into contenteditable.
  Check Unicode, line breaks and retained bold formatting.
- Repeat drags and cancellations; each started sequence should have one final
  callback, with no growing list of active sources.

Stop with Ctrl-C. Generated fixtures remain available for inspection; remove
only the printed probe directory when finished.

## Coverage boundary

This example currently runs on macOS. It uses a minimal NSView-backed window
handle shim to call the exact native adapter independently of the main app.
It does **not** verify Tauri IPC, the Svelte gesture controller, nonactivating
clipboard window policy, preparation tokens, export leases or privacy guards.
Those require separate application integration tests. Embedded WebKit also
does not prove compatibility with external Chromium or Firefox. The drop
fixture itself works in browsers on all platforms.

AppKit does not expose a public forced-cancel method for `NSDraggingSession`.
The macOS adapter observes the cancellation flag before starting and when
AppKit asks for allowed operations. Native Escape cancellation must therefore
be tested independently of cancellation-flag tests.

## Recorded runs

On macOS 26.5.2 the probe and production AppKit adapter compiled in an isolated
debug Cargo harness. Both payload validation tests passed without Tokio. The
localhost page loaded in Edge and in the embedded WKWebView.

A native CUA drag from the PNG source to embedded WebKit reached the source's
`mouseDragged` path, but the observed global pressed-button state was already
zero:

```text
NATIVE_START sequence=1 kind=0 mouse_buttons=0
NATIVE_END sequence=1 outcome=Cancelled
```

The production guard correctly rejected that gesture before starting a native
session. No browser drop report was produced. The observation does not identify
whether this comes from event timing or synthetic-input state semantics. The
available one-shot CUA drag API exposes neither a held-button step nor duration,
so no native upload success is claimed for this run. Manual PNG/file/text/HTML,
Escape and delayed-read checks remain required. The temporary Edge tab was
closed; no unrelated browser page or clipboard data was changed.

Do not treat compilation, a synthetic DOM event, or the fixture's own JavaScript
as evidence that native dragging works. In particular, this cancelled startup
does not test the AppKit ending-delegate lifecycle.
