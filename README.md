# ArcRelay Desktop

ArcRelay Desktop is the open-source desktop client for ArcRelay, a local-first
device collaboration system. It provides clipboard sharing, nearby file
transfer, cross-screen input, printer sharing, local automation, and desktop
system integration.

The official ArcRelay mobile application is distributed separately and its
source code is not included in this repository.

## Development

Requirements:

- Rust stable
- Node.js 22
- Platform development tools required by Tauri 2

Install dependencies and run the checks:

```sh
npm ci
npm run check
npm test
cargo fmt --all -- --check
cargo test --all-targets
```

Start the desktop development server with:

```sh
npm run tauri -- dev
```

Create a community build with:

```sh
npm run bundle
```

Screenshot capture and OCR use an optional proprietary sidecar in official
ArcRelay builds. Community builds work without that sidecar. Maintainers can
provide prebuilt sidecars through `SNIPTRA_ARTIFACT_DIR`; official packaging
uses `npm run bundle:official` and requires `ARCRELAY_REQUIRE_SNIPTRA=1`.

## Versions and releases

All desktop package manifests use one SemVer value. Check them with
`npm run version:check`; update them together with
`npm run version:set -- 0.2.0`. A `v0.2.0` tag must point to a commit whose
manifests report `0.2.0`.

Pushing a matching `v*` tag runs the GitHub release workflow for macOS,
Windows, and Linux and uploads the packages to a draft GitHub Release. A
maintainer reviews the draft before publishing it.

## Documentation

The [action text format](docs/action-text-format.md) describes the portable JSON
format accepted by the desktop client. Development QA artifacts, product design
sources, and internal test reports are intentionally kept outside the public
repository.

## License

Source code in this repository is licensed under the GNU Affero General Public
License, version 3 only. See [LICENSE](LICENSE).

The ArcRelay name, logos, and other brand assets are not granted under the
software license. See the project-level trademark policy before distributing a
modified build under ArcRelay branding.
