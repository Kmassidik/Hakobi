from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from PIL import Image

REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from tools.cutout import cutout_image  # noqa: E402

MANIFEST = Path(__file__).with_name("art_manifest.json")


def load_presets() -> dict:
    return json.loads(MANIFEST.read_text())


def slice_sheet(
    sheet: Path,
    names: list[str],
    output_dir: Path,
    *,
    size: int = 256,
    columns: int = 3,
    rows: int = 2,
    padding: float = 0.08,
    tolerance: int = 18,
    remove_background: bool = True,
) -> list[Path]:
    if not names:
        raise ValueError("at least one output name is required")
    if len(names) > columns * rows:
        raise ValueError(f"{len(names)} names exceed the {columns * rows}-cell sheet")
    if not 0 <= padding < 0.5:
        raise ValueError("padding must be between 0 and 0.5")

    with Image.open(sheet) as source:
        image = source.convert("RGBA")

    cell_width = image.width // columns
    cell_height = image.height // rows
    if cell_width == 0 or cell_height == 0:
        raise ValueError("sheet is smaller than the requested grid")

    output_dir.mkdir(parents=True, exist_ok=True)
    inner = max(1, round(size * (1 - 2 * padding)))
    written: list[Path] = []

    for index, name in enumerate(names):
        row, column = divmod(index, columns)
        cell = image.crop(
            (
                column * cell_width,
                row * cell_height,
                (column + 1) * cell_width,
                (row + 1) * cell_height,
            )
        )
        try:
            subject = cutout_image(
                cell,
                remove_background=remove_background,
                tolerance=tolerance,
            )
        except ValueError as error:
            raise ValueError(f"cell {index + 1} ({name}) is empty: {error}") from error

        subject.thumbnail((inner, inner), Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        canvas.alpha_composite(
            subject,
            ((size - subject.width) // 2, (size - subject.height) // 2),
        )
        destination = output_dir / f"{name}.webp"
        canvas.save(destination, "WEBP", quality=90, method=6)
        written.append(destination)

    return written


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Slice a 3x2 ChatGPT art sheet into transparent WebP assets."
    )
    parser.add_argument("sheet", type=Path, help="Input PNG sheet")
    parser.add_argument("--preset", help="Preset name from tools/art_manifest.json")
    parser.add_argument("--names", nargs="+", help="Output names, in cell order")
    parser.add_argument("--out", type=Path, help="Output directory")
    parser.add_argument("--size", type=int, help="Final square size in pixels")
    parser.add_argument("--columns", type=int)
    parser.add_argument("--rows", type=int)
    parser.add_argument("--padding", type=float, default=0.08)
    parser.add_argument("--tolerance", type=int, default=18)
    parser.add_argument(
        "--keep-background",
        action="store_true",
        help="Do not remove a flat background; only trim alpha.",
    )
    args = parser.parse_args(argv)

    presets = load_presets()
    preset = presets.get(args.preset or "", {})
    names = args.names or preset.get("names")
    if not names:
        parser.error("use --preset or provide --names")
    out = args.out or (preset.get("out") if args.preset else None)
    if out is None:
        parser.error("use --out or a preset with an output directory")
    size = args.size or int(preset.get("size", 256))
    columns = args.columns or int(preset.get("columns", 3))
    rows = args.rows or int(preset.get("rows", 2))

    output_dir = Path(out)
    if not output_dir.is_absolute():
        output_dir = REPO_ROOT / output_dir

    try:
        written = slice_sheet(
            args.sheet,
            names,
            output_dir,
            size=size,
            columns=columns,
            rows=rows,
            padding=args.padding,
            tolerance=args.tolerance,
            remove_background=not args.keep_background,
        )
    except Exception as error:
        print(f"slice_sheets: {error}", file=sys.stderr)
        return 1

    for path in written:
        try:
            shown = path.relative_to(REPO_ROOT)
        except ValueError:
            shown = path
        print(shown)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
