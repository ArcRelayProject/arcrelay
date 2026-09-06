"""Exercise release/version validation and read back real Finder metadata.

No application compilation, disk-image creation or mounting is performed.
"""

import copy
from datetime import datetime, timezone
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest

from ds_store import DSStore
from mac_alias import Alias, Bookmark, TargetInfo, VolumeInfo

from build_dmg import layout_settings, read_config, verify_volume


class DmgLayoutTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.volume = Path(self.directory.name)
        self.config = read_config()
        self.app = self.volume / (self.config["productName"] + ".app")
        (self.app / "Contents").mkdir(parents=True)
        self.write_info(self.config["version"])

    def write_info(self, version):
        (self.app / "Contents/Info.plist").write_bytes(plistlib.dumps({
            "CFBundleIdentifier": self.config["identifier"],
            "CFBundleShortVersionString": version,
        }))

    def prepare_volume(self):
        (self.volume / "Applications").symlink_to("/Applications")
        background = self.volume / ".background.tiff"
        background.touch()
        settings = layout_settings(self.config, self.app)
        # The release image uses HFS+. Build its 32-bit catalog identifiers
        # explicitly; APFS temp directories can have unsupported 64-bit IDs.
        created = datetime(2026, 1, 1, tzinfo=timezone.utc)
        alias = Alias(
            volume=VolumeInfo("ArcRelay", created, b"H+", 0, 0, b"\x00\x00"),
            target=TargetInfo(0, background.name, 2, 16, created, b"\x00" * 4, b"TIFF"),
        )
        with DSStore.open(str(self.volume / ".DS_Store"), "w+") as store:
            store["."]["icvp"] = {
                "backgroundType": 2,
                "backgroundImageAlias": alias.to_bytes(),
                "iconSize": settings["icon_size"],
            }
            width, height = settings["window_rect"][1]
            store["."]["bwsp"] = {
                "WindowBounds": "{{100, 100}, {%d, %d}}" % (width, height),
                "ShowToolbar": False,
            }
            for name, position in settings["icon_locations"].items():
                store[name]["Iloc"] = position

    def test_rejects_stale_application_before_packaging(self):
        self.write_info("0.0.0")
        with self.assertRaisesRegex(ValueError, "version"):
            layout_settings(self.config, self.app)

    def test_rejects_another_application(self):
        config = copy.deepcopy(self.config)
        config["identifier"] = "com.example.other"
        with self.assertRaisesRegex(ValueError, "identifier"):
            layout_settings(config, self.app)

    def test_layout_contains_only_app_and_applications_link(self):
        settings = layout_settings(self.config, self.app)
        self.assertEqual(settings["files"], [(str(self.app.resolve()), self.app.name)])
        self.assertEqual(settings["symlinks"], {"Applications": "/Applications"})
        self.assertEqual(settings["hide_extensions"], [])
        self.assertNotIn(self.app.name, settings.get("hide", []))

    @unittest.skipUnless(sys.platform == "darwin", "requires macOS codesign and SetFile")
    def test_finder_settings_preserve_an_existing_code_signature(self):
        # Use an ad-hoc signed fixture, never a developer identity or keychain.
        # This is not an application build or a disk-image packaging test.
        executable = self.app / "Contents/MacOS/probe"
        executable.parent.mkdir()
        shutil.copyfile("/usr/bin/true", executable)
        executable.chmod(0o755)
        info_path = self.app / "Contents/Info.plist"
        info = plistlib.loads(info_path.read_bytes())
        info.update(CFBundleExecutable="probe", CFBundlePackageType="APPL")
        info_path.write_bytes(plistlib.dumps(info))
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", "--timestamp=none", str(self.app)],
                       check=True, capture_output=True, timeout=15)
        settings = layout_settings(self.config, self.app)
        # dmgbuild applies these flags to the copied, already signed app.
        for option, flag in [("hide_extensions", "E"), ("hide", "V")]:
            for name in settings.get(option, []):
                subprocess.run(["/usr/bin/SetFile", "-a", flag, str(self.volume / name)],
                               check=True, capture_output=True, timeout=15)
        result = subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(self.app)],
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_verifies_serialized_finder_layout(self):
        self.prepare_volume()
        verify_volume(self.config, self.volume)

    def test_rejects_missing_background_in_packaged_volume(self):
        self.prepare_volume()
        (self.volume / ".background.tiff").unlink()
        with self.assertRaisesRegex(ValueError, "missing file"):
            verify_volume(self.config, self.volume)

    def test_rejects_legacy_bookmark_that_hides_background_in_finder_26(self):
        self.prepare_volume()
        bookmark = Bookmark.for_file(str(self.volume / ".background.tiff"))
        with DSStore.open(str(self.volume / ".DS_Store"), "r+") as store:
            store["."]["pBBk"] = bookmark
        with self.assertRaisesRegex(ValueError, "Finder-incompatible"):
            verify_volume(self.config, self.volume)

    def test_rejects_reset_icon_positions(self):
        self.prepare_volume()
        with DSStore.open(str(self.volume / ".DS_Store"), "r+") as store:
            store["Applications"]["Iloc"] = (0, 0)
        with self.assertRaisesRegex(ValueError, "icon position"):
            verify_volume(self.config, self.volume)


if __name__ == "__main__":
    unittest.main()
