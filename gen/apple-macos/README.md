# ArcRelay macOS Xcode project

This project makes Xcode the final macOS build layer while keeping the Tauri
and Rust application source in `arcrelay-desktop`.

## Development

Install XcodeGen once:

```bash
brew install xcodegen
```

From `arcrelay-desktop`, run:

```bash
npm run tauri:macos:dev
```

The command starts (or reuses) the Vite server, regenerates the Xcode project,
and opens it. Press Command-R in Xcode to compile and launch the Debug build.

## App icon

`AppIcon.icon` is the Icon Composer source. Xcode compiles it into both an
adaptive `Assets.car` representation and a fallback `AppIcon.icns` for older
macOS releases. Edit the source by opening `AppIcon.icon` in Icon Composer,
then rebuild in Xcode.

## Release

The Release configuration invokes `tauri build --no-bundle` for both Apple
Silicon and Intel, combines the Rust binaries with `lipo`, and lets Xcode own
the final app bundle, signing, Archive, and distribution steps.
