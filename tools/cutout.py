from __future__ import annotations

import argparse
import sys
from collections import Counter, deque
from pathlib import Path

from PIL import Image


def _has_transparency(image: Image.Image, threshold: float = 0.01) -> bool:
    alpha = image.getchannel("A")
    histogram = alpha.histogram()
    transparent = sum(histogram[:250])
    return transparent > image.width * image.height * threshold


def _border_pixels(image: Image.Image):
    width, height = image.size
    pixels = image.load()
    for x in range(width):
        yield pixels[x, 0]
        yield pixels[x, height - 1]
    for y in range(1, height - 1):
        yield pixels[0, y]
        yield pixels[width - 1, y]


def _background_colors(image: Image.Image, limit: int = 4) -> list[tuple[int, int, int]]:
    """Return the most common border colours, quantized to survive PNG noise."""
    counts: Counter[tuple[int, int, int]] = Counter()
    for pixel in _border_pixels(image):
        key = (pixel[0] // 8 * 8, pixel[1] // 8 * 8, pixel[2] // 8 * 8)
        counts[key] += 1
    minimum = max(4, (image.width + image.height) // 50)
    return [key for key, count in counts.most_common(limit) if count >= minimum]


def _is_background(pixel, colours, tolerance: int) -> bool:
    for colour in colours:
        if (
            abs(pixel[0] - colour[0]) <= tolerance
            and abs(pixel[1] - colour[1]) <= tolerance
            and abs(pixel[2] - colour[2]) <= tolerance
        ):
            return True
    return False


def remove_flat_background(
    image: Image.Image,
    tolerance: int = 18,
) -> Image.Image:
    """Remove a uniform border-connected background by flood fill.

    This is intentionally conservative: it only removes pixels connected to the
    image border. It handles white, black, and flat checkerboard backgrounds,
    but does not pretend to segment complex photographic scenes. For difficult
    images, use a dedicated subject-segmentation model instead.
    """
    image = image.convert("RGBA")
    colours = _background_colors(image)
    if not colours:
        raise ValueError("could not detect a flat border background")

    width, height = image.size
    pixels = image.load()
    seen = bytearray(width * height)
    queue: deque[tuple[int, int]] = deque()

    def push(x: int, y: int) -> None:
        index = y * width + x
        if seen[index]:
            return
        seen[index] = 1
        if _is_background(pixels[x, y], colours, tolerance):
            queue.append((x, y))

    for x in range(width):
        push(x, 0)
        push(x, height - 1)
    for y in range(1, height - 1):
        push(0, y)
        push(width - 1, y)

    while queue:
        x, y = queue.popleft()
        pixels[x, y] = (pixels[x, y][0], pixels[x, y][1], pixels[x, y][2], 0)
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < width and 0 <= ny < height:
                push(nx, ny)

    return image


def cutout_image(
    image: Image.Image,
    *,
    remove_background: bool = True,
    tolerance: int = 18,
) -> Image.Image:
    """Normalize alpha, optionally remove a flat background, then trim."""
    image = image.convert("RGBA")
    if remove_background and not _has_transparency(image):
        image = remove_flat_background(image, tolerance=tolerance)

    alpha = image.getchannel("A")
    bbox = alpha.getbbox()
    if bbox is None:
        raise ValueError("image contains no visible pixels after cutout")
    return image.crop(bbox)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Cut out and trim a PNG.")
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--keep-background",
        action="store_true",
        help="Only trim alpha; do not remove a flat background.",
    )
    parser.add_argument("--tolerance", type=int, default=18)
    args = parser.parse_args(argv)

    with Image.open(args.input) as source:
        result = cutout_image(
            source,
            remove_background=not args.keep_background,
            tolerance=args.tolerance,
        )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    result.save(args.output)
    print(f"{args.output} ({result.width}x{result.height}, rgba)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"cutout: {error}", file=sys.stderr)
        raise SystemExit(1)
