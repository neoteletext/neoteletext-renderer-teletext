# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests of scripts/arabic-sets.py (REQ-2164, REQ-2166), run by the push gate:

    uv run scripts/test_arabic_sets.py

They need the pinned ArabicShaping.txt, which the generator downloads once to
its cache and checks by SHA-256.
"""

import importlib.util
import shutil
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("arabic_sets", HERE / "arabic-sets.py")
arabic_sets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(arabic_sets)


def committed_text():
    """The generated text between the marker lines of the committed file."""
    text = arabic_sets.TARGET.read_text()
    return text.split(arabic_sets.BEGIN)[1].split(arabic_sets.END)[0]


def private_points(allocation):
    """The private-use part of an allocation, by (letter, form)."""
    start, end = arabic_sets.PRIVATE_FORMS_START, arabic_sets.PRIVATE_FORMS_END
    return {key: point for key, point in allocation.items() if start <= ord(point) <= end}


def generated_allocation(text):
    """Every (letter, form) and the code point a generated text gives it."""
    return {
        (chr(int(match[1], 16)), match[2].lower()): chr(int(match[3], 16))
        for match in arabic_sets.JOINED_FORM_ROW.finditer(text)
    }


class CheckMode(unittest.TestCase):
    """REQ-2164: `--check` passes on the committed tables, and fails, naming
    the line, on tables changed by hand."""

    def setUp(self):
        self.folder = Path(tempfile.mkdtemp())
        self.copy = self.folder / "arabic_script.rs"
        shutil.copy(arabic_sets.TARGET, self.copy)

    def tearDown(self):
        shutil.rmtree(self.folder)

    def test_the_committed_tables_pass(self):
        arabic_sets.main(["--check", "--target", str(self.copy)])

    def test_a_table_changed_by_hand_fails_and_changes_nothing(self):
        text = self.copy.read_text()
        edited = text.replace("JoiningType::None), // ARABIC LETTER HAMZA\n", "JoiningType::Dual), // ARABIC LETTER HAMZA\n", 1)
        self.assertNotEqual(edited, text, "the committed tables list the hamza as non-joining")
        self.copy.write_text(edited)
        with self.assertRaises(SystemExit) as failure:
            arabic_sets.main(["--check", "--target", str(self.copy)])
        self.assertIn("ARABIC LETTER HAMZA", str(failure.exception.code))
        self.assertEqual(self.copy.read_text(), edited, "--check writes nothing")


class AppendOnlyAllocation(unittest.TestCase):
    """REQ-2166 item 2: the committed allocation is a prefix of a fresh one."""

    def setUp(self):
        self.sets = arabic_sets.SETS
        self.committed = arabic_sets.committed_allocation(committed_text())

    def tearDown(self):
        arabic_sets.SETS = self.sets

    def test_the_committed_allocation_is_reproduced(self):
        fresh = generated_allocation(arabic_sets.generated(self.committed))
        self.assertEqual(private_points(fresh), self.committed)

    def test_a_letter_inserted_before_the_others_takes_the_next_code_point(self):
        # U+063B, KEHEH WITH TWO DOTS ABOVE: dual-joining, with no
        # presentation forms, and before every letter allocated today.
        new_letter = "\u063b"
        self.assertLess(new_letter, min(letter for letter, _ in self.committed))
        name, option, letters, digits, stop = arabic_sets.SETS[0]
        arabic_sets.SETS = [(name, option, new_letter + letters, digits, stop), *arabic_sets.SETS[1:]]
        fresh = private_points(generated_allocation(arabic_sets.generated(self.committed)))
        self.assertEqual({key: fresh[key] for key in self.committed}, self.committed, "no committed code point moves")
        highest = max(ord(point) for point in self.committed.values())
        added = {key: point for key, point in fresh.items() if key not in self.committed}
        self.assertEqual(
            added,
            {
                (new_letter, "final"): chr(highest + 1),
                (new_letter, "initial"): chr(highest + 2),
                (new_letter, "medial"): chr(highest + 3),
            },
        )

    def test_a_withdrawn_form_keeps_its_code_point(self):
        balochi = [index for index, entry in enumerate(arabic_sets.SETS) if entry[0] == "Balochi"][0]
        name, option, letters, digits, stop = arabic_sets.SETS[balochi]
        withdrawn = "\u08a9"
        sets = list(arabic_sets.SETS)
        sets[balochi] = (name, option, letters.replace(withdrawn, ""), digits, stop)
        arabic_sets.SETS = sets
        fresh = private_points(generated_allocation(arabic_sets.generated(self.committed)))
        self.assertEqual(fresh, self.committed, "the withdrawn letter's forms keep their rows")

    def test_the_range_is_never_overrun(self):
        full = {("\u0628", "final"): chr(arabic_sets.PRIVATE_FORMS_END)}
        with self.assertRaises(SystemExit):
            arabic_sets.allocate([("\u06ae", "final")], {}, full)

    def test_every_code_point_is_in_the_range_with_one_meaning(self):
        points = list(self.committed.values())
        self.assertEqual(len(points), len(set(points)))
        for point in points:
            self.assertTrue(arabic_sets.PRIVATE_FORMS_START <= ord(point) <= arabic_sets.PRIVATE_FORMS_END)


if __name__ == "__main__":
    unittest.main()
