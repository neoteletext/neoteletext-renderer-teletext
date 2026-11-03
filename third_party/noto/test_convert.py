# /// script
# requires-python = ">=3.11"
# dependencies = ["freetype-py==2.5.1", "uharfbuzz==0.56.3", "numpy==2.4.6"]
# ///
"""Tests of the converter's drawing rules (REQ-2165), run by the push gate:

    uv run third_party/noto/test_convert.py

They draw with a stand-in face whose ink is a solid block the size it is
asked for, so they need no Noto source.
"""

import importlib.util
import sys
import unittest
from pathlib import Path

import numpy as np

# Loading the module under test leaves no __pycache__ beside it, which
# `cargo xtask check-noto` would refuse in the font folder.
sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("convert", HERE / "convert.py")
convert = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(convert)


class BlockFace:
    """A face whose every glyph is the outline of a box `across` columns
    wide and `lines` lines tall, standing on the canvas's baseline line: a
    clipped glyph loses an edge, a smaller one keeps all four."""

    cmap = {ord("X")}

    def draw(self, text, across, lines, rtl=False):
        canvas = np.zeros((convert.MARGIN * 2, convert.MARGIN * 2), dtype=bool)
        height, width = int(round(lines)), int(round(across))
        canvas[convert.MARGIN - height : convert.MARGIN, convert.MARGIN : convert.MARGIN + width] = True
        canvas[convert.MARGIN - height + 1 : convert.MARGIN - 1, convert.MARGIN + 1 : convert.MARGIN + width - 1] = False
        return canvas


class BlockScript:
    """A script drawing from `BlockFace` at the size it is given."""

    def __init__(self, lines, across, baseline=16):
        self.lines, self.across, self.baseline = lines, across, baseline
        self.rtl = self.join = False
        self.faces = [BlockFace()]

    def face_for(self, character):
        return next((face for face in self.faces if ord(character) in face.cmap), None)


class ShrinkRule(unittest.TestCase):
    """REQ-2165: a glyph that would leave the cell is drawn smaller, never
    clipped: all its ink lies inside the 12 by 20 cell, on the baseline."""

    def assert_inside(self, cell, script):
        self.assertIsNotNone(cell)
        self.assertEqual(cell.shape, (convert.CELL_HEIGHT, convert.CELL_WIDTH))
        top, bottom, left, right = convert.extent(cell)
        self.assertEqual(bottom, script.baseline, "the ink stands on the baseline")
        ink = cell[top:bottom, left:right]
        edges = (ink[0].all(), ink[-1].all(), ink[:, 0].all(), ink[:, -1].all())
        self.assertEqual(edges, (True, True, True, True), "every edge of the box is drawn: nothing is clipped")
        return bottom - top, right - left

    def test_a_glyph_that_fits_is_drawn_at_its_size(self):
        script = BlockScript(lines=10, across=8)
        self.assertEqual(self.assert_inside(convert.draw_glyph(script, "X"), script), (10, 8))

    def test_a_glyph_too_tall_is_drawn_shorter(self):
        script = BlockScript(lines=30, across=8)
        height, width = self.assert_inside(convert.draw_glyph(script, "X"), script)
        self.assertLessEqual(height, script.baseline)
        self.assertEqual(width, 8, "only the height shrinks")

    def test_a_glyph_too_wide_is_drawn_narrower(self):
        script = BlockScript(lines=10, across=20)
        height, width = self.assert_inside(convert.draw_glyph(script, "X"), script)
        self.assertLessEqual(width, convert.CELL_WIDTH)
        self.assertEqual(height, 10, "only the width shrinks")

    def test_a_glyph_too_large_both_ways_is_drawn_smaller_both_ways(self):
        script = BlockScript(lines=40, across=30)
        height, width = self.assert_inside(convert.draw_glyph(script, "X"), script)
        self.assertLessEqual((height, width), (script.baseline, convert.CELL_WIDTH))

    def test_a_character_no_face_has_is_not_drawn(self):
        self.assertIsNone(convert.draw_glyph(BlockScript(lines=10, across=8), "Y"))


if __name__ == "__main__":
    unittest.main()
