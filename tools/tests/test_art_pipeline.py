from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools.cutout import cutout_image, remove_flat_background
from tools.slice_sheets import slice_sheet

SHEET_SIZE = (1536, 1024)
CELL = 512
COLOURS = [
    (22, 79, 59, 255),
    (216, 240, 120, 255),
    (46, 116, 87, 255),
    (255, 196, 87, 255),
    (24, 33, 29, 255),
    (220, 222, 215, 255),
]


def make_sheet(path: Path, *, transparent: bool) -> None:
    background = (0, 0, 0, 0) if transparent else (255, 255, 255, 255)
    image = Image.new("RGBA", SHEET_SIZE, background)
    for index, colour in enumerate(COLOURS):
        row, column = divmod(index, 3)
        left = column * CELL + 106
        top = row * CELL + 106
        image.alpha_composite(
            Image.new("RGBA", (300, 300), colour),
            (left, top),
        )
    image.save(path)


class ArtPipelineTests(unittest.TestCase):
    def test_slices_transparent_sheet(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sheet = root / "sheet.png"
            make_sheet(sheet, transparent=True)
            names = [f"icon-{index}" for index in range(6)]

            written = slice_sheet(sheet, names, root / "out", size=256)

            self.assertEqual(len(written), 6)
            for path in written:
                with Image.open(path) as image:
                    self.assertEqual(image.size, (256, 256))
                    self.assertEqual(image.mode, "RGBA")
                    self.assertLess(image.getchannel("A").getextrema()[0], 255)

    def test_removes_flat_white_background(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sheet = root / "sheet.png"
            make_sheet(sheet, transparent=False)

            cell = Image.open(sheet).crop((0, 0, CELL, CELL))
            removed = remove_flat_background(cell)
            self.assertEqual(removed.getpixel((0, 0))[3], 0)

            subject = cutout_image(cell, remove_background=True)
            self.assertLess(subject.width, CELL)
            self.assertLess(subject.height, CELL)
            alpha = subject.getchannel("A")
            self.assertEqual(alpha.getextrema()[1], 255)

    def test_empty_cell_fails_clearly(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sheet = root / "sheet.png"
            image = Image.new("RGBA", SHEET_SIZE, (0, 0, 0, 0))
            image.alpha_composite(
                Image.new("RGBA", (300, 300), COLOURS[0]),
                (106, 106),
            )
            image.save(sheet)

            with self.assertRaisesRegex(ValueError, "cell 2"):
                slice_sheet(sheet, ["one", "two"], root / "out")


if __name__ == "__main__":
    unittest.main()
