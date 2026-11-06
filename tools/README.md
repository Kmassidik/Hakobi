# Hakobi art pipeline

Small local pipeline for turning ChatGPT sheet output into Hakobi WebP assets.

Requirements:

```sh
python3 -m pip install -r tools/requirements.txt
```

## Slice a sheet

The prompt pack defines presets in `tools/art_manifest.json`.

```sh
python3 tools/slice_sheets.py raw/A1-build-ship.png --preset A1
python3 tools/slice_sheets.py raw/A2-operate.png --preset A2
python3 tools/slice_sheets.py raw/A3-observe-protect.png --preset A3
python3 tools/slice_sheets.py raw/M1-hako.png --preset M1
python3 tools/slice_sheets.py raw/E1-empty-states.png --preset E1
```

Or specify names directly:

```sh
python3 tools/slice_sheets.py raw/A1-build-ship.png \
  --names source-repo build-runner container-image stack-compose deploy rollback \
  --out static/assets/art/3d --size 256
```

The tool:

- crops a 3x2 sheet into 512px cells
- accepts transparent sheets directly
- removes a flat border-connected white/black/checkerboard background
- trims each subject
- fits it into a square with 8% transparent padding
- writes `name.webp` with alpha
- refuses empty cells instead of writing blank assets

For generated sheets that already have perfect alpha, use `--keep-background` to
skip the background-removal heuristic.

## Cut out a single image

```sh
python3 tools/cutout.py raw/hero-pipeline.png static/assets/art/hero-pipeline.webp
```

This is useful for hero, login, and other one-off images. Output format follows
the file extension.

## Limitations

The cutout is conservative flood-fill, not an ML segmentation model. It works
for the flat backgrounds requested in `docs/ASSET_PROMPT_PACK.md`. For a
photographic or gradient background, use a dedicated segmentation tool and
feed the resulting transparent PNG into the slicer.

## Tests

```sh
python3 -m unittest discover -s tools/tests -v
```
