#!/usr/bin/env python3
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

SCRIPTS_DIR = Path(__file__).resolve().parents[1] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

import validate_pptx as MODULE

PRESENTATION_XML = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
                xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
                xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst>
  <p:sldSz cx="12192000" cy="6858000"/>
</p:presentation>"""

PRESENTATION_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
</Relationships>"""


def write_minimal_pptx(
    path: Path,
    slide_xml: str,
    notes_xml: str | None = None,
    notes_target: str = "../notesSlides/notesSlide1.xml",
) -> None:
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", "<Types/>")
        z.writestr("ppt/presentation.xml", PRESENTATION_XML)
        z.writestr("ppt/_rels/presentation.xml.rels", PRESENTATION_RELS)
        z.writestr("ppt/slides/slide1.xml", slide_xml)
        if notes_xml is not None:
            slide_rels = (
                '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
                '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
                '<Relationship Id="rId1" '
                'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" '
                f'Target="{notes_target}"/>'
                "</Relationships>"
            )
            z.writestr("ppt/slides/_rels/slide1.xml.rels", slide_rels)
            z.writestr("ppt/notesSlides/notesSlide1.xml", notes_xml)


def make_slide_xml(inner_sp_tree: str) -> str:
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
        'xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">'
        f"<p:cSld><p:spTree>{inner_sp_tree}</p:spTree></p:cSld></p:sld>"
    )


NOTES_XML = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
         xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld><p:spTree><p:sp>
    <p:nvSpPr><p:cNvPr id="2" name="Notes"/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
    <p:txBody><a:p><a:r><a:t>Speaker note</a:t></a:r></a:p></p:txBody>
  </p:sp></p:spTree></p:cSld>
</p:notes>"""


class ValidatePptxTests(unittest.TestCase):
    def test_leaked_tags_detection(self):
        regex = MODULE.LEAKED_TAG_REGEX
        self.assertIsNotNone(regex.search('Source: <a href="https://example.com">Link</a>'))
        self.assertIsNotNone(regex.search("Items: <ul><li>Item 1</li></ul>"))
        self.assertIsNotNone(regex.search("<ol><li>First</li></ol>"))
        self.assertIsNotNone(regex.search("Leaked raw tag <p> inside compiled text"))
        # The compiler passes attribute-carrying <b>/<strong> through verbatim;
        # both spellings must be caught when they leak into DrawingML text.
        self.assertIsNotNone(regex.search('<b style="color:#B83A2C">96.2%</b>'))
        self.assertIsNotNone(regex.search('<strong style="color:#009682;">x</strong>'))
        self.assertIsNotNone(regex.search("italic <em>word</em>"))
        self.assertIsNone(regex.search("Plain text without any HTML tags"))

    def test_validation_report_properties(self):
        report = MODULE.ValidationReport(file_path="test.pptx", is_valid=True, slide_count=1)
        self.assertEqual(report.error_count, 0)
        self.assertEqual(report.warning_count, 0)

        report.issues.append(MODULE.SlideIssue(slide_index=1, severity="warning", category="test", message="warn"))
        self.assertEqual(report.error_count, 0)
        self.assertEqual(report.warning_count, 1)

        report.issues.append(MODULE.SlideIssue(slide_index=1, severity="error", category="test", message="err"))
        self.assertEqual(report.error_count, 1)
        self.assertEqual(report.warning_count, 1)

    def test_validator_missing_file_raises(self):
        with self.assertRaises(FileNotFoundError):
            MODULE.PptxValidator(Path("/path/to/missing.pptx"))

    @patch.object(MODULE.sys, "platform", "linux")
    @patch("shutil.which", return_value=None)
    def test_render_thumbnails_cleans_stale_files(self, _mock_which):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_dir = Path(tmp)
            preview_dir = tmp_dir / "preview"
            preview_dir.mkdir()
            (preview_dir / "slide-01.png").write_text("old")
            (preview_dir / "slide-99.png").write_text("stale")
            (preview_dir / "contact-sheet.jpg").write_text("stale-cs")
            dummy_pptx = tmp_dir / "dummy.pptx"
            dummy_pptx.write_bytes(b"PK\x05\x06" + b"\x00" * 18)
            validator = MODULE.PptxValidator(dummy_pptx)
            validator.render_thumbnails(preview_dir)
            self.assertFalse((preview_dir / "slide-01.png").exists())
            self.assertFalse((preview_dir / "slide-99.png").exists())
            self.assertFalse((preview_dir / "contact-sheet.jpg").exists())

    def test_validate_valid_slide_and_notes_targets(self):
        slide_xml = make_slide_xml(
            "<p:sp>"
            '<p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>'
            '<p:txBody><a:p><a:r><a:rPr sz="2800"/><a:t>Quarterly Review</a:t></a:r></a:p></p:txBody>'
            "</p:sp>"
        )
        for target in ("../notesSlides/notesSlide1.xml", "/ppt/notesSlides/notesSlide1.xml"):
            with self.subTest(notes_target=target), tempfile.TemporaryDirectory() as tmp:
                pptx = Path(tmp) / "valid.pptx"
                write_minimal_pptx(pptx, slide_xml, notes_xml=NOTES_XML, notes_target=target)
                report = MODULE.PptxValidator(pptx).validate(expected_pages=1, check_notes=True)
                self.assertTrue(report.is_valid)
                self.assertEqual(report.slide_count, 1)
                self.assertEqual(report.error_count, 0)
                self.assertEqual(report.warning_count, 0)
                self.assertEqual(report.slides[0].title, "Quarterly Review")
                self.assertTrue(report.slides[0].has_notes)
                self.assertEqual(report.slides[0].notes_text, "Speaker note")

    def test_validate_page_mismatch_sets_is_valid_false(self):
        slide_xml = make_slide_xml("")
        with tempfile.TemporaryDirectory() as tmp:
            pptx = Path(tmp) / "mismatch.pptx"
            write_minimal_pptx(pptx, slide_xml)
            report = MODULE.PptxValidator(pptx).validate(expected_pages=2)
            self.assertFalse(report.is_valid)
            self.assertEqual(report.error_count, 1)
            self.assertEqual(report.issues[0].category, "structure")

    def test_validate_detects_leaked_html_in_table_cell(self):
        slide_xml = make_slide_xml(
            "<p:graphicFrame>"
            '<p:nvGraphicFramePr><p:cNvPr id="3" name="MetricsTable"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>'
            "<a:graphic><a:graphicData><a:tbl><a:tr><a:tc>"
            "<a:txBody><a:p><a:r><a:t>&lt;ul&gt;&lt;li&gt;Leak&lt;/li&gt;&lt;/ul&gt;</a:t></a:r></a:p></a:txBody>"
            "</a:tc></a:tr></a:tbl></a:graphicData></a:graphic>"
            "</p:graphicFrame>"
        )
        with tempfile.TemporaryDirectory() as tmp:
            pptx = Path(tmp) / "table_leak.pptx"
            write_minimal_pptx(pptx, slide_xml)
            report = MODULE.PptxValidator(pptx).validate()
            self.assertFalse(report.is_valid)
            self.assertEqual(report.error_count, 1)
            self.assertEqual(report.slides[0].tables_count, 1)
            self.assertEqual(report.issues[0].category, "leaked_tag")
            self.assertEqual(report.issues[0].element_id, "MetricsTable")

    def test_validate_missing_notes_warns(self):
        slide_xml = make_slide_xml("")
        with tempfile.TemporaryDirectory() as tmp:
            pptx = Path(tmp) / "no_notes.pptx"
            write_minimal_pptx(pptx, slide_xml)
            report = MODULE.PptxValidator(pptx).validate(check_notes=True)
            self.assertTrue(report.is_valid)
            self.assertEqual(report.error_count, 0)
            self.assertEqual(report.warning_count, 1)
            self.assertEqual(report.issues[0].category, "notes")

    def test_validate_bad_zip_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            bad_pptx = Path(tmp) / "corrupt.pptx"
            bad_pptx.write_bytes(b"not a zip archive")
            report = MODULE.PptxValidator(bad_pptx).validate()
            self.assertFalse(report.is_valid)
            self.assertEqual(report.error_count, 1)
            self.assertEqual(report.issues[0].category, "structure")


if __name__ == "__main__":
    unittest.main()
