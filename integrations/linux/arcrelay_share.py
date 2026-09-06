import subprocess

from gi.repository import Gio, GObject, Nautilus


ARCRELAY_EXECUTABLE = @ARCRELAY_EXE_JSON@


class ArcRelayShareExtension(GObject.GObject, Nautilus.MenuProvider):
    def get_file_items(self, *args):
        files = args[-1]
        paths = [item.get_location().get_path() for item in files]
        if not paths or any(path is None or item.get_file_type() != Gio.FileType.REGULAR for path, item in zip(paths, files)):
            return []

        item = Nautilus.MenuItem(
            name="ArcRelayShareExtension::share",
            label="Share with ArcRelay",
            tip="Send selected files to a nearby device",
            icon="arcrelay-desktop",
        )
        item.connect("activate", self._share, paths)
        return [item]

    @staticmethod
    def _share(_menu, paths):
        subprocess.Popen(
            [
                ARCRELAY_EXECUTABLE,
                "--arcrelay-share-source",
                "linux-file-manager",
                "--arcrelay-share",
                *paths,
            ],
            close_fds=True,
        )
