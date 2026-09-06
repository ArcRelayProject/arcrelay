# Bundled fonts

ArcRelay bundles the complete variable build of **Noto Sans SC** for consistent
interface typography across supported desktop platforms.

- Source: <https://github.com/google/fonts/tree/main/ofl/notosanssc>
- Upstream file: `NotoSansSC[wght].ttf`
- Supported weight axis: 100–900
- Bundled format: WOFF2
- Glyph coverage: complete upstream font; no character subsetting
- License: SIL Open Font License 1.1, included in `OFL.txt`

The WOFF2 file is a lossless web-font conversion of the upstream variable TTF.
System emoji and symbol fonts remain in the CSS fallback stack for characters
that are not provided by Noto Sans SC.
