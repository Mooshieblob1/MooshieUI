import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("release_artifacts", Path(__file__).with_name("release_artifacts.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseArtifactsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.source = Path(self.temp.name) / "source"
        self.source.mkdir()
        self.output = Path(self.temp.name) / "output"
        for name in ("MooshieUI_2.3.1_amd64.AppImage", "MooshieUI_2.3.1_x64-setup.exe", "MooshieUI.app.tar.gz"):
            (self.source / name).write_bytes(b"signed archive content")
            (self.source / (name + ".sig")).write_text("signature")
        (self.source / "MooshieUI_2.3.1_aarch64.dmg").write_bytes(b"dmg")

    def collect(self, macos=True):
        return module.collect(self.source, self.output, "v2.3.1", "example/app", macos)

    def test_mac_updater_renaming_preserves_signed_bytes(self):
        result = self.collect()
        self.assertIn("darwin-aarch64", result["platforms"])
        self.assertTrue(result["platforms"]["darwin-aarch64"]["url"].endswith("MooshieUI_2.3.1_aarch64.app.tar.gz"))
        self.assertEqual((self.output / "MooshieUI_2.3.1_aarch64.app.tar.gz").read_bytes(), b"signed archive content")
        self.assertIn("latest.json", (self.output / "SHA256SUMS").read_text())

    def test_unqualified_mac_assets_are_excluded(self):
        result = self.collect(macos=False)
        self.assertNotIn("darwin-aarch64", result["platforms"])
        self.assertFalse(list(self.output.glob("*.dmg")))
        self.assertFalse(list(self.output.glob("*.tar.gz")))

    def test_missing_mac_signature_blocks_release(self):
        (self.source / "MooshieUI.app.tar.gz.sig").unlink()
        with self.assertRaises(FileNotFoundError):
            self.collect()

    def test_missing_or_wrong_version_installer_blocks_release(self):
        (self.source / "MooshieUI_2.3.1_aarch64.dmg").rename(self.source / "MooshieUI_2.3.0_aarch64.dmg")
        with self.assertRaises(ValueError):
            self.collect()

    def test_empty_signature_blocks_release(self):
        (self.source / "MooshieUI.app.tar.gz.sig").write_text("")
        with self.assertRaises(ValueError):
            self.collect()


if __name__ == "__main__":
    unittest.main()
