# Windows clipboard focus contract

The chooser is a nonactivating, topmost WebView window. Opening it, scrolling,
selecting a record, using its inline action menu and inserting a record must not
activate ArcRelay. Windows ignores saved `clipboardAutoFocusSearch=true`; other
platforms retain their behavior. Explicit search (click or Ctrl+F), text editing
and label input temporarily allow activation and use native WebView/IME input.
Entering editing can end Explorer's rename operation; restoring a top-level
window does not reconstruct a destroyed rename edit control.

## Implementation boundaries

- `windowing/clipboard_windows_policy.rs` is pure session/shortcut policy and
  has ordinary tests requiring no Tokio context.
- `windowing/clipboard_windows.rs` owns native styles, showing, keyboard/mouse
  hooks, foreground events and transaction recipient validation.
  Its navigation/editing IPC is restricted to the clipboard WebView.
- Native showing/hiding intentionally bypasses Tao's cached visibility flags.
  Changing Tao flags can call an activating `ShowWindow` as a side effect. The
  builder starts nonfocusable; editing toggles the native `WS_EX_NOACTIVATE`
  style and explicitly focuses the WebView. Do not replace these operations with
  unconditional Tauri `show`, `set_focusable` or `set_focus` calls.
- One lazily installed hook/message thread survives hidden and destroyed
  WebViews until application exit. Hidden, unready, editing and pinned-idle
  sessions pass input through. Already consumed key releases are drained even
  after hiding. This avoids hook installation/uninstallation and keyup races.
- Hook callbacks use nonblocking state access and bounded event delivery; UI,
  clipboard work and IPC happen outside the callbacks. Injected keys are never
  intercepted; Ctrl+V remains owned by the existing continuous-paste trigger.
- Each presentation has a generation and waits for frontend readiness. Old
  events cannot operate a reopened window. Foreground/window/input-control
  changes stop interception. Pinned windows enter passive mode after insertion
  or outside interaction; clicking inside explicitly starts a new session.
- Windows action menus render inside the chooser, including nested format,
  label and peer actions. No native popup menu owner can take foreground focus.
- Each insertion captures its own HWND, PID, thread and focused child. After
  clipboard writeback and UI handoff, changed/closed targets cancel insertion.
  Physical modifiers must release before simulated paste; failed content stays
  on the clipboard. Foreground restoration is conditional, never forced after
  the user has switched applications. Higher-integrity targets can reject input.

## Automated checks

Run `npm run check`, `npm run test:clipboard` and:

```sh
npx playwright test --config playwright.visual.config.ts clipboard-interactions.spec.ts clipboard-windows.spec.ts
cargo test -p arcrelay-desktop --bin arcrelay-desktop windowing::clipboard
```

The Windows CI job compiles the actual desktop target and runs native show,
reopen and noactivate-style tests without an async runtime. Browser preview tests
use `?preview-platform=windows` in development only; they test event routing,
focus-mode intent and menus, not Windows/WebView2 system focus or IME behavior.

## Interactive Windows acceptance (not replaced by browser/native-unit tests)

1. Explorer: select a disposable file and press F2. Open the clipboard shortcut
   on first use, then again after hiding, then after the 60-second idle teardown.
   The rename edit control and selection must remain intact every time.
2. With rename active, test mouse selection, wheel scrolling, arrows, paging,
   Ctrl+1, Enter and Shift+Enter. Plain text must enter the filename edit control.
3. Right-click a record, enter the paste-format submenu and insert plain text.
   Also verify disabled formats, labels and paired-peer menus without activation.
4. Drag/resize the chooser. Repeat on monitors with negative origins and
   100/150/200% scaling; verify positioning, menu containment and outside clicks.
5. Click search / Ctrl+F and enter Chinese IME text. Test input-to-input handoff,
   Escape, Tab back to results, text editing, tag editing and preview selection.
   These explicit editing operations are allowed to leave the original app.
6. Pin the chooser, insert, and type arrows/Enter in the destination. They must
   not be intercepted until clicking inside the chooser again. Alt+Tab must not
   force the old recipient back to foreground. Close/change the destination
   during writeback; insertion must fail safely, preserving clipboard content.
7. Verify continuous Ctrl+V, held modifiers, key repeats and rapid reopen do not
   produce duplicate insertion, stuck modifiers or stale actions. Test ordinary
   and elevated target apps without automatically elevating ArcRelay.
8. Recheck macOS/Linux search defaults, native menus, pinning and clipboard paste.
