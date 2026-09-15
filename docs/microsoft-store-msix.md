# Microsoft Store MSIX packages

ArcRelay's Store packages are produced by the **Build Microsoft Store MSIX** GitHub Actions workflow. The workflow builds native x64 and ARM64 packages and combines them into `ArcRelay.msixbundle`. The final `microsoft-store-msix` artifact also contains both architecture-specific `.msix` files and SHA-256 checksums.

## One-time Partner Center setup

Reserve the ArcRelay product name in Partner Center, open **Product identity**, and create these GitHub repository variables with the values shown there:

- `MSIX_IDENTITY_NAME`: Package/Identity/Name
- `MSIX_PUBLISHER`: Package/Identity/Publisher
- `MSIX_PUBLISHER_DISPLAY_NAME`: the publisher display name associated with the Store account

These values are public package metadata, not secrets. They must match Partner Center exactly or Store ingestion will reject the package.

## Build and submit

The workflow runs automatically for `v*` tags and can also be started at **Actions → Build Microsoft Store MSIX → Run workflow**. Normally, leave `package_version` empty. The workflow converts the application's SemVer version into a Store-compatible version by adding one to the major component and setting the Store-reserved fourth component to zero. For example, ArcRelay `0.2.0` becomes package version `1.2.0.0`; this preserves ordering when the application reaches `1.0.0`.

Use the optional version input only when an already-submitted Partner Center package requires a higher version. It must be four integers, the first must be non-zero, every component must be at most 65535, and the fourth must be zero.

Download the `microsoft-store-msix` workflow artifact and submit `ArcRelay.msixbundle` in Partner Center. The package is intentionally unsigned: Microsoft Store signs it after certification. The individual `.msix` files are also retained for diagnosis.

The Store package includes the ArcRelay executable, Tauri resources, both Sniptra sidecars, and the Windows Share Target as one package identity. It does not include the nested sparse Share Target package used by the standalone NSIS/MSI installers.

Before the first public submission, run the Windows App Certification Kit against the downloaded bundle and test install the Store-signed flight on both x64 and ARM64 Windows 11 devices.
