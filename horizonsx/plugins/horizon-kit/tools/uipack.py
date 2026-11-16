#!/usr/bin/env python3
"""Builds a plugin's menu pack from a folder of drawings (for horizon_kit::ui).

    SRC/
      art/<name>.png        a menu's picture: the window is at (left, top) inside it (176 wide, 114 + rows*18 tall)
      art/<name>.json       optional: {"left": 104, "top": 26, "rows": 4}   (defaults 0, 0, 6)
      items/<name>.png      16x16 item pictures: item model <ns>:<name>  (gui item: item::model(stack, "ns:name"))
      pack.png              optional pack icon

    python uipack.py SRC OUT_DIR --namespace myplugin [--description "..."]

writes OUT_DIR/<ns>-pack.zip, OUT_DIR/ui.json (the arts by name, for ui::Arts::load) and prints the sha1. The pack
holds the shared kitui fonts (pixel moves, text at any row, the invisible item) from tools/kitui_base.

Limits checked here: a picture must fit a 427 x 240 GUI (1440p at GUI scale 6): the window is centred, so `left`
and the width right of the window each up to 125, `top` up to (240 - window height) / 2.
"""
import argparse
import hashlib
import io
import json
import pathlib
import sys
import zipfile

from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
BASE = HERE / 'kitui_base'


def last_column(im):
    for x in range(im.width - 1, -1, -1):
        for y in range(im.height):
            if im.getpixel((x, y))[3]:
                return x + 1
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('src')
    ap.add_argument('out')
    ap.add_argument('--namespace', required=True)
    ap.add_argument('--description', default='Menus')
    a = ap.parse_args()
    src, out, ns = pathlib.Path(a.src), pathlib.Path(a.out), a.namespace.lower()
    out.mkdir(parents=True, exist_ok=True)
    files = {}
    if not BASE.exists():
        sys.exit('kitui_base is missing: run tools/ui_fonts_gen.py first')
    for f in BASE.rglob('*'):
        if f.is_file():
            files[f.relative_to(BASE).as_posix()] = f.read_bytes()
    providers, arts, problems = [], {}, []
    for i, png in enumerate(sorted((src / 'art').glob('*.png'))):
        name = png.stem
        im = Image.open(png).convert('RGBA')
        meta = {}
        side = png.with_suffix('.json')
        if side.exists():
            meta = json.loads(side.read_text(encoding='utf-8'))
        left, top, rows = int(meta.get('left', 0)), int(meta.get('top', 0)), int(meta.get('rows', 6))
        h = 114 + rows * 18
        if left > 125 or im.width - left - 176 > 125:
            problems.append(f'{name}: wider than a 1440p screen allows (left {left}, right {im.width - left - 176}; max 125 each)')
        if top > (240 - h) // 2:
            problems.append(f'{name}: {top} px above a {rows}-row window, max {(240 - h) // 2}')
        ch = chr(0xE000 + i)
        files[f'assets/{ns}/textures/font/art_{name}.png'] = png.read_bytes()
        providers.append({'type': 'bitmap', 'file': f'{ns}:font/art_{name}.png', 'ascent': 13 + top, 'height': im.height, 'chars': [ch]})
        arts[name] = {'font': f'{ns}:art', 'ch': ch, 'left': left, 'top': top, 'advance': last_column(im) + 1, 'rows': rows}
    if problems:
        print('\n'.join(problems))
        sys.exit(1)
    files[f'assets/{ns}/font/art.json'] = json.dumps({'providers': providers}, indent=1).encode()
    for png in sorted((src / 'items').glob('*.png')) if (src / 'items').exists() else []:
        n = png.stem
        files[f'assets/{ns}/textures/item/{n}.png'] = png.read_bytes()
        files[f'assets/{ns}/models/item/{n}.json'] = json.dumps({'parent': 'minecraft:item/generated', 'textures': {'layer0': f'{ns}:item/{n}'}}).encode()
        files[f'assets/{ns}/items/{n}.json'] = json.dumps({'model': {'type': 'minecraft:model', 'model': f'{ns}:item/{n}'}}).encode()
    if (src / 'pack.png').exists():
        files['pack.png'] = (src / 'pack.png').read_bytes()
    files['pack.mcmeta'] = json.dumps({'pack': {'description': a.description, 'min_format': [88, 0], 'max_format': [97, 1]}}).encode()
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, 'w', zipfile.ZIP_DEFLATED) as z:
        for n in sorted(files):
            info = zipfile.ZipInfo(n, date_time=(1980, 1, 2, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, files[n])
    data = buf.getvalue()
    (out / f'{ns}-pack.zip').write_bytes(data)
    (out / 'ui.json').write_text(json.dumps(arts, ensure_ascii=False, indent=1), encoding='utf-8')
    print(f'{len(arts)} arts, {len(files)} files, {len(data)} bytes, sha1 {hashlib.sha1(data).hexdigest()}')


if __name__ == '__main__':
    main()
