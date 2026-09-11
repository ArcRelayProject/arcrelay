# Desktop screenshot capture

The local screenshot suite renders every desktop WebView surface at the logical
content size used by its Tauri window. It is intentionally not wired into CI.

Run the complete eight-language, light/dark matrix:

```bash
npm run screenshots
```

Generated PNGs, JSON metadata, and the HTML contact sheet are written beneath
`visual-artifacts/actual/` and ignored by Git. Override the output directory or
select one dimension with environment variables:

```bash
ARCRELAY_SCREENSHOT_DIR=visual-artifacts/review \
ARCRELAY_SCREENSHOT_FEATURE=input \
ARCRELAY_SCREENSHOT_LANGUAGE=jaJp \
ARCRELAY_SCREENSHOT_THEME=dark \
npm run screenshots
```

The dimensions are sourced from the Tauri window declarations:

- main window: 1180 x 760
- clipboard: 540 x 820
- permission guide: 430 x 800
- tray transfer: 440 x 220, expanding to 440 x 520 when content is present
- privacy overlay reference target: 1180 x 760

The browser is used as a deterministic WebView driver with the application's
existing mock bridges. `visual=1` is required for the stable language, theme,
font, image, and animation readiness behavior; normal app behavior is unchanged.
