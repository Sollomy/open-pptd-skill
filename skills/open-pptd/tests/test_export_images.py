#!/usr/bin/env python3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

import export_images as MODULE


class ExportImagesTests(unittest.TestCase):
    def test_build_montage_empty_list(self):
        with tempfile.TemporaryDirectory() as td:
            out_file = Path(td) / "overview.jpg"
            MODULE.build_montage([], out_file)
            self.assertFalse(out_file.exists())

    def test_build_montage_creates_grid(self):
        try:
            from PIL import Image
        except ImportError:
            self.skipTest("Pillow not installed")

        with tempfile.TemporaryDirectory() as td:
            d = Path(td)
            img1 = d / "slide_1.png"
            img2 = d / "slide_2.png"
            im = Image.new("RGB", (800, 600), color="white")
            im.save(img1)
            im.save(img2)

            out_file = d / "overview.jpg"
            MODULE.build_montage([img1, img2], out_file, cols=2)
            self.assertTrue(out_file.exists())
            with Image.open(out_file) as res:
                self.assertGreater(res.width, 0)
                self.assertGreater(res.height, 0)

    def test_export_images_missing_source(self):
        with self.assertRaises(FileNotFoundError):
            MODULE.export_images(Path("/path/to/nonexistent/deck.pptd"))

    @patch.object(MODULE, "build_contact_sheet")
    @patch("subprocess.run")
    @patch("shutil.which", side_effect=lambda name: f"/usr/bin/{name}" if name in ("soffice", "pdftoppm") else None)
    @patch.object(MODULE, "export_pptx")
    def test_export_images_isolates_deck_ignores_out_dir_cleans_stale_and_renames(
        self, mock_export, _mock_which, mock_run, mock_contact_sheet
    ):
        with tempfile.TemporaryDirectory() as td:
            src_dir = Path(td) / "deck"
            (src_dir / "pages").mkdir(parents=True)
            manifest = src_dir / "deck.pptd"
            manifest.write_text("pages: [pages/01.page]\n")
            (src_dir / "pages" / "01.page").write_text("elements: []\n")

            out_dir = src_dir / "qa-images"
            out_dir.mkdir()
            (out_dir / "slide-01.png").write_text("old-1")
            (out_dir / "slide-99.png").write_text("stale-99")
            (out_dir / "contact-sheet.jpg").write_text("stale-cs")

            def fake_export(isolated_manifest, output, force=False, pptd_bin=None):
                self.assertNotEqual(isolated_manifest.parent.resolve(), src_dir.resolve())
                self.assertFalse((isolated_manifest.parent / "qa-images").exists())
                # Simulate compiler writing .sync.hash inside isolated copy only
                (isolated_manifest.parent / ".sync.hash").write_text("hash")
                output.write_bytes(b"pptx")
                return output

            def fake_run(cmd, **_kwargs):
                if cmd[0].endswith("soffice"):
                    outdir = Path(cmd[cmd.index("--outdir") + 1])
                    (outdir / "render.pdf").write_bytes(b"%PDF")
                elif cmd[0].endswith("pdftoppm"):
                    # Stale renders must already be deleted before pdftoppm runs
                    self.assertFalse((out_dir / "slide-01.png").exists())
                    self.assertFalse((out_dir / "slide-99.png").exists())
                    self.assertFalse((out_dir / "contact-sheet.jpg").exists())
                    prefix = Path(cmd[-1])
                    Path(f"{prefix}-1.png").write_bytes(b"png1")
                    Path(f"{prefix}-2.png").write_bytes(b"png2")
                return subprocess.CompletedProcess(cmd, 0)

            mock_export.side_effect = fake_export
            mock_run.side_effect = fake_run

            result = MODULE.export_images(src_dir)
            self.assertEqual(result, out_dir.resolve())
            self.assertFalse((src_dir / ".sync.hash").exists())
            self.assertFalse((out_dir / "slide-99.png").exists())
            self.assertFalse((out_dir / "slide-1.png").exists())
            self.assertTrue((out_dir / "slide-01.png").is_file())
            self.assertTrue((out_dir / "slide-02.png").is_file())
            mock_contact_sheet.assert_called_once_with(
                [out_dir.resolve() / "slide-01.png", out_dir.resolve() / "slide-02.png"],
                out_dir.resolve() / "contact-sheet.jpg",
            )

    @patch.object(MODULE.sys, "platform", "linux")
    @patch("subprocess.run")
    @patch("shutil.which", return_value=None)
    @patch.object(MODULE, "export_pptx")
    def test_export_images_missing_soffice_fallback(self, mock_export, _mock_which, mock_run):
        with tempfile.TemporaryDirectory() as td:
            src_dir = Path(td) / "deck"
            src_dir.mkdir()
            (src_dir / "deck.pptd").write_text("pages: []\n")

            result = MODULE.export_images(src_dir)
            self.assertEqual(result, (src_dir / "qa-images").resolve())
            mock_export.assert_called_once()
            mock_run.assert_not_called()

    @patch.object(MODULE, "export_images")
    def test_main_force_flag_defaults_false(self, mock_export_images):
        self.assertEqual(MODULE.main(["deck.pptd"]), 0)
        self.assertFalse(mock_export_images.call_args.kwargs["force"])
        self.assertEqual(MODULE.main(["deck.pptd", "--force"]), 0)
        self.assertTrue(mock_export_images.call_args.kwargs["force"])


if __name__ == "__main__":
    unittest.main()
