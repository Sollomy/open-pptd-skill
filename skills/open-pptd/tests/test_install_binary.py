#!/usr/bin/env python3
import hashlib
import io
import os
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

import install_binary as MODULE


def make_version_script(path: Path, output: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"#!{sys.executable}\nprint({output!r})\n")
    path.chmod(0o755)
    return path


def make_tar_gz(member_name: str, payload: bytes) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        info = tarfile.TarInfo(name=member_name)
        info.size = len(payload)
        tar.addfile(info, io.BytesIO(payload))
    return buf.getvalue()


class _FakeResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


class InstallBinaryTests(unittest.TestCase):
    def test_find_pptd_ignores_npm_wrapper_and_accepts_native(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td).resolve()
            npm_bin = root / "npm_bin"
            fake_home = root / "home"
            fake_cwd = root / "cwd"
            fake_cwd.mkdir()

            # npm's open-pptd wrapper prints bare semver ("1.4.2"), not "pptd 1.4.2"
            make_version_script(npm_bin / "open-pptd", "1.4.2")
            make_version_script(npm_bin / "pptd", "1.4.2")

            real_probe = MODULE.probe_binary_version

            def isolated_probe(p: Path):
                try:
                    p.resolve().relative_to(root)
                except ValueError:
                    return None
                return real_probe(p)

            env = {**os.environ, "PATH": str(npm_bin)}
            with (
                patch.dict(os.environ, env, clear=True),
                patch.object(Path, "home", return_value=fake_home),
                patch.object(Path, "cwd", return_value=fake_cwd),
                patch.object(MODULE, "probe_binary_version", side_effect=isolated_probe),
            ):
                self.assertIsNone(MODULE.find_pptd(min_version="1.4.2"))
                self.assertIsNone(MODULE.find_pptd())

                native = make_version_script(fake_home / ".local" / "bin" / "pptd", "pptd 1.4.2")
                self.assertEqual(MODULE.find_pptd(min_version="1.4.2"), native.resolve())

    def test_install_from_release_rejects_sha256_mismatch(self):
        archive_name = "pptd-1.4.2-x86_64-linux.tar.gz"
        archive_bytes = make_tar_gz("pptd", b"fake-binary")

        with tempfile.TemporaryDirectory() as td:
            dest_dir = Path(td) / "bin"
            with (
                patch.object(MODULE, "detect_target", return_value=("x86_64-unknown-linux-gnu", [archive_name])),
                patch("urllib.request.urlopen", return_value=_FakeResponse(archive_bytes)),
                patch.object(MODULE, "fetch_checksums", return_value={archive_name: "0" * 64}),
            ):
                with self.assertRaisesRegex(RuntimeError, "SHA256 checksum mismatch"):
                    MODULE.install_from_release(dest_dir, version="1.4.2")
                self.assertFalse((dest_dir / "pptd").exists())

    def test_install_from_release_overwrites_regular_open_pptd_copy(self):
        archive_name = "pptd-1.4.2-x86_64-linux.tar.gz"
        payload = b"new-pptd-binary"
        archive_bytes = make_tar_gz("pptd", payload)
        digest = hashlib.sha256(archive_bytes).hexdigest()

        with tempfile.TemporaryDirectory() as td:
            dest_dir = Path(td) / "bin"
            dest_dir.mkdir()
            stale_copy = dest_dir / "open-pptd"
            stale_copy.write_bytes(b"stale-binary")

            with (
                patch.object(MODULE.platform, "system", return_value="Linux"),
                patch.object(MODULE, "detect_target", return_value=("x86_64-unknown-linux-gnu", [archive_name])),
                patch("urllib.request.urlopen", return_value=_FakeResponse(archive_bytes)),
                patch.object(MODULE, "fetch_checksums", return_value={archive_name: digest}),
            ):
                installed = MODULE.install_from_release(dest_dir, version="1.4.2")
                self.assertEqual(installed, dest_dir / "pptd")
                self.assertEqual(installed.read_bytes(), payload)
                self.assertEqual(stale_copy.read_bytes(), payload)

    def test_install_from_cargo_custom_dir_not_ending_in_bin(self):
        with tempfile.TemporaryDirectory() as td:
            dest_dir = Path(td) / "tools"

            def fake_cargo_run(cmd, check=False):
                root = Path(cmd[cmd.index("--root") + 1])
                built = root / "bin" / "pptd"
                built.parent.mkdir(parents=True, exist_ok=True)
                built.write_bytes(b"cargo-built-pptd")
                return subprocess.CompletedProcess(cmd, 0)

            with (
                patch.object(MODULE.platform, "system", return_value="Linux"),
                patch("shutil.which", return_value="/usr/bin/cargo"),
                patch("subprocess.run", side_effect=fake_cargo_run),
            ):
                installed = MODULE.install_from_cargo(dest_dir, version="1.4.2")
                self.assertEqual(installed, dest_dir / "pptd")
                self.assertTrue(installed.is_file())
                self.assertEqual(installed.read_bytes(), b"cargo-built-pptd")
                self.assertTrue(installed.stat().st_mode & stat.S_IXUSR)


if __name__ == "__main__":
    unittest.main()
