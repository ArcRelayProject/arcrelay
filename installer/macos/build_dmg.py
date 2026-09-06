"""Build/verify Finder layouts without requiring a GUI session on the CI host.

The application is built and signed by Tauri. This script only packages that
existing bundle; the CI signs and notarizes the resulting DMG afterward.
"""

import argparse
import json
import os
from pathlib import Path
import plistlib
import tempfile


PROJECT = Path(__file__).resolve().parents[2]
ICON_SIZE = 96


def read_config(project=PROJECT):
    return json.loads((project / "tauri.conf.json").read_text())


def layout_settings(config, app, project=PROJECT):
    app = Path(app).resolve()
    with (app / "Contents/Info.plist").open("rb") as source:
        info = plistlib.load(source)
    if info.get("CFBundleIdentifier") != config["identifier"]:
        raise ValueError("DMG input has the wrong application identifier")
    if info.get("CFBundleShortVersionString") != config["version"]:
        raise ValueError("DMG input version does not match the release configuration")
    name = config["productName"] + ".app"
    dmg = config["bundle"]["macOS"]["dmg"]
    background = project / dmg["background"]
    if not background.is_file():
        raise ValueError("DMG background is missing")
    if not background.with_name(background.stem + "@2x" + background.suffix).is_file():
        raise ValueError("DMG Retina background is missing")
    size = dmg["windowSize"]
    positions = {
        name: tuple(dmg["appPosition"][axis] for axis in ("x", "y")),
        "Applications": tuple(dmg["applicationFolderPosition"][axis] for axis in ("x", "y")),
    }
    return {
        "format": "UDZO",
        "filesystem": "HFS+",
        "files": [(str(app), name)],
        "symlinks": {"Applications": "/Applications"},
        "icon": str(project / "icons/icon.icns"),
        "background": str(background),
        "window_rect": ((100, 100), (size["width"], size["height"])),
        "icon_locations": positions,
        "icon_size": ICON_SIZE,
        "text_size": 13,
        # dmgbuild implements this with SetFile -a E, which adds FinderInfo
        # to the already signed app and makes codesign --strict reject it.
        # Keep Finder customization in the volume's .DS_Store, not the app.
        "hide_extensions": [],
        "default_view": "icon-view",
        "include_icon_view_settings": True,
        "include_list_view_settings": False,
        "show_status_bar": False,
        "show_tab_view": False,
        "show_toolbar": False,
        "show_pathbar": False,
        "show_sidebar": False,
        "arrange_by": None,
    }


def verify_volume(config, volume):
    from ds_store import DSStore
    from mac_alias import Alias

    volume = Path(volume)
    name = config["productName"] + ".app"
    expected = layout_settings(config, volume / name)
    applications = volume / "Applications"
    if not applications.is_symlink() or os.readlink(applications) != "/Applications":
        raise ValueError("DMG Applications link is missing or points to the wrong location")
    with DSStore.open(str(volume / ".DS_Store"), "r") as store:
        if any(entry.filename == "." and entry.code == b"pBBk" for entry in store):
            raise ValueError("DMG contains a Finder-incompatible background bookmark")
        for item, position in expected["icon_locations"].items():
            if store[item]["Iloc"] != position:
                raise ValueError(f"DMG icon position is incorrect: {item}")
        view = store["."]["icvp"]
        if view.get("backgroundType") != 2 or view.get("iconSize") != ICON_SIZE:
            raise ValueError("DMG background or icon sizing was not applied")
        alias = Alias.from_bytes(view["backgroundImageAlias"])
        if alias.target.filename != ".background.tiff":
            raise ValueError("DMG does not use the combined Retina background")
        if not (volume / alias.target.filename).is_file():
            raise ValueError("DMG background alias points to a missing file")
        bounds = expected["window_rect"]
        expected_bounds = "{{%d, %d}, {%d, %d}}" % (*bounds[0], *bounds[1])
        window = store["."]["bwsp"]
        if window["WindowBounds"] != expected_bounds or window["ShowToolbar"]:
            raise ValueError("DMG window geometry was not applied")
    print("Verified Finder-compatible DMG background, Retina asset, window, icon positions and Applications link")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify-volume", type=Path)
    args = parser.parse_args()
    config = read_config()
    if args.verify_volume:
        if args.app or args.output:
            parser.error("--verify-volume cannot be combined with build arguments")
        verify_volume(config, args.verify_volume)
        return
    if not args.app or not args.output:
        parser.error("--app and --output are required to build a DMG")
    settings = layout_settings(config, args.app)
    output = args.output.resolve()
    if output.exists():
        raise FileExistsError(f"Refusing to overwrite an existing DMG: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    from dmgbuild import build_dmg

    with tempfile.TemporaryDirectory(prefix=".arcrelay-dmg-", dir=output.parent) as staging:
        temporary = Path(staging) / output.name
        build_dmg(str(temporary), config["productName"], settings=settings, lookForHiDPI=True)
        temporary.replace(output)
    print(f"Created {output}")


if __name__ == "__main__":
    main()
