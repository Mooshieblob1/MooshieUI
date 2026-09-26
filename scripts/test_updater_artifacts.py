import base64
import contextlib
import importlib.util
import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("updater_artifacts", Path(__file__).with_name("updater_artifacts.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

KEY_ID = bytes(range(1, 9))


def fake_signature(name, key_id=KEY_ID):
    """A structurally valid Tauri `.sig` (random bytes in place of real signatures)."""
    b64 = lambda data: base64.b64encode(data).decode()
    box = (
        "untrusted comment: signature from tauri secret key\n"
        f"{b64(b'ED' + key_id + os.urandom(64))}\n"
        f"trusted comment: timestamp:1790000000\tfile:{name}\n"
        f"{b64(os.urandom(64))}\n"
    )
    return base64.b64encode(box.encode()).decode()


class UpdaterArtifactsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "artifacts"
        self.files = [
            self.source / "appimage/MooshieUI_2.3.1_amd64.AppImage",
            self.source / "deb/MooshieUI_2.3.1_amd64.deb",
            self.source / "rpm/MooshieUI-2.3.1-1.x86_64.rpm",
            self.source / "nsis/MooshieUI_2.3.1_x64-setup.exe",
            self.source / "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/MooshieUI.app.tar.gz",
        ]
        for file in self.files:
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_bytes(b"installer")
        # Look-alikes outside the bundle directories Tauri signs from.
        (self.source / "appimage/MooshieUI.AppDir/usr/bin").mkdir(parents=True)
        (self.source / "appimage/MooshieUI.AppDir/usr/bin/tool.AppImage").write_bytes(b"x")
        (self.source / "nsis/MooshieUI_2.3.1_x64-setup.exe.sig").write_text("stale")
        pubkey_box = "untrusted comment: minisign public key: X\n" + base64.b64encode(
            b"Ed" + KEY_ID + bytes(32)).decode() + "\n"
        self.config = self.root / "tauri.conf.json"
        self.config.write_text(json.dumps(
            {"plugins": {"updater": {"pubkey": base64.b64encode(pubkey_box.encode()).decode()}}}))

    def sign_all(self, key_id=KEY_ID):
        for file in self.files:
            Path(f"{file}.sig").write_text(fake_signature(file.name, key_id))

    def check(self, macos=True):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            code = module.check(self.source, macos, self.config)
        return code, out.getvalue()

    def test_lists_exactly_what_tauri_signed(self):
        self.assertEqual(module.updater_files(self.source, macos=True), self.files)
        self.assertEqual(module.updater_files(self.source, macos=False), self.files[:4])

    def test_missing_or_duplicate_bundle_fails(self):
        (self.source / "nsis/Other_x64-setup.exe").write_bytes(b"x")
        with self.assertRaises(SystemExit):
            module.updater_files(self.source, macos=False)
        self.files[0].unlink()
        with self.assertRaises(SystemExit):
            module.updater_files(self.source, macos=False)

    def test_every_signature_present_passes(self):
        self.sign_all()
        code, out = self.check()
        self.assertEqual(code, 0)
        self.assertNotIn("::warning::", out)

    def test_missing_signature_fails_the_release(self):
        self.sign_all()
        Path(f"{self.files[4]}.sig").unlink()
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("missing signature", out)
        # Without the Mac build the Mac archive is not expected.
        self.assertEqual(self.check(macos=False)[0], 0)

    def test_malformed_or_misnamed_signature_fails(self):
        self.sign_all()
        Path(f"{self.files[3]}.sig").write_text("stale")
        self.assertEqual(self.check()[0], 1)
        Path(f"{self.files[3]}.sig").write_text(fake_signature("other.exe"))
        self.assertEqual(self.check()[0], 1)

    def test_other_key_only_warns_like_tauri(self):
        self.sign_all(key_id=bytes(8))
        code, out = self.check()
        self.assertEqual(code, 0)
        self.assertIn("::warning::", out)

    def test_macos_archive_matches_tauri_layout(self):
        bundle = self.root / "bundle"
        app = bundle / "macos/MooshieUI.app"
        (app / "Contents/MacOS").mkdir(parents=True)
        binary = app / "Contents/MacOS/mooshieui"
        binary.write_bytes(b"\xcf\xfa\xed\xfe binary")
        binary.chmod(0o755)
        (app / "Contents/Info.plist").write_text("<plist/>")
        os.link(app / "Contents/Info.plist", app / "Contents/Info-copy.plist")
        (app / "Contents/Current").symlink_to("MacOS")
        archive = module.macos_archive(bundle)
        self.assertEqual(archive, bundle / "macos/MooshieUI.app.tar.gz")
        with tarfile.open(archive) as tar:
            members = {m.name: m for m in tar.getmembers()}
            self.assertEqual(tar.getmembers()[0].name, "MooshieUI.app")
            self.assertTrue(all(name.split("/")[0] == "MooshieUI.app" for name in members))
            self.assertTrue(members["MooshieUI.app/Contents/Current"].issym())
            self.assertTrue(members["MooshieUI.app/Contents/Info-copy.plist"].isreg())
            self.assertEqual(members["MooshieUI.app/Contents/MacOS/mooshieui"].mode & 0o777, 0o755)
            self.assertEqual(tar.extractfile("MooshieUI.app/Contents/MacOS/mooshieui").read(),
                             binary.read_bytes())


if __name__ == "__main__":
    unittest.main()
