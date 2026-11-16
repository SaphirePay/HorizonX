#!/usr/bin/env python3
"""Makes the kit's menu-text data from the game's own font (run once per game version):

  horizon-kit/src/ui_widths.rs   every character's advance in the default font (for exact centring/alignment)
  tools/kitui_base/              the shared pack part every drawn menu needs:
      assets/kitui/font/space.json      pixel moves: \\uF801.. = -1,-2,-4..-1024, \\uF821.. = +1,+2,+4..+1024
      assets/kitui/font/y<N>.json       the default font with its top at window row N (N = 5..240)
      assets/kitui/items/blank.json     an invisible item (clickable areas over drawn buttons)

The line fonts point at the game's own textures (minecraft:font/*.png); nothing of the game is copied.

    python ui_fonts_gen.py [path to the game's assets/minecraft]
"""
import json
import pathlib
import sys

from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
GAME = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path(
    r'%USERPROFILE%\AppData\Roaming\horizon-launcher\server-runtime\game-files\horizon-client\26.3\assets\minecraft')
BASE = HERE / 'kitui_base'
# tools/ beside the crate (plugin-tools/kit) or inside it (plugins/horizon-kit/tools)
WIDTHS = (HERE.parent / 'src' if (HERE.parent / 'src').exists() else HERE.parent / 'horizon-kit' / 'src') / 'ui_widths.rs'
TOP, BOTTOM = 5, 240


def providers():
    """The default font's providers in the game's order: spaces, then the bitmaps."""
    out = []
    default = json.loads((GAME / 'font' / 'default.json').read_text(encoding='utf-8'))
    for p in default['providers']:
        if p['type'] == 'reference':
            ref = p['id'].split(':')[-1]
            inc = json.loads((GAME / 'font' / f'{ref}.json').read_text(encoding='utf-8'))
            out += [q for q in inc['providers'] if q['type'] in ('space', 'bitmap')]
        elif p['type'] in ('space', 'bitmap'):
            out.append(p)
    return out


def advances(provs):
    adv = {}
    for p in provs:
        if p['type'] == 'space':
            for ch, a in p['advances'].items():
                adv.setdefault(ch, int(a))
            continue
        tex = p['file'].split(':')[-1]
        img = Image.open(GAME / 'textures' / tex).convert('RGBA')
        rows = p['chars']
        cw, chh = img.width // len(rows[0]), img.height // len(rows)
        height = p.get('height', 8)
        scale = height / chh
        for r, row in enumerate(rows):
            for c, ch in enumerate(row):
                if ch in adv or ch == '\u0000':
                    continue
                cell = img.crop((c * cw, r * chh, (c + 1) * cw, (r + 1) * chh))
                w = 0
                for x in range(cw - 1, -1, -1):
                    if any(cell.getpixel((x, y))[3] for y in range(chh)):
                        w = x + 1
                        break
                adv[ch] = int(0.5 + w * scale) + 1
    return adv


def main():
    provs = providers()
    adv = advances(provs)
    # Rust table, sorted for a binary search.
    items = sorted((ord(ch), a) for ch, a in adv.items() if len(ch) == 1)
    lines = ['//! Written by tools/ui_fonts_gen.py from the game\'s default font: each character\'s advance in pixels.',
             '//! Do not edit.', '', f'pub(crate) static ADVANCES: [(u32, u8); {len(items)}] = [']
    row = []
    for code, a in items:
        row.append(f'({code}, {a})')
        if len(row) == 12:
            lines.append('    ' + ', '.join(row) + ',')
            row = []
    if row:
        lines.append('    ' + ', '.join(row) + ',')
    lines.append('];')
    WIDTHS.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    # The base pack part.
    font_dir = BASE / 'assets' / 'kitui' / 'font'
    font_dir.mkdir(parents=True, exist_ok=True)
    space = {}
    for k in range(11):
        space[chr(0xF801 + k)] = -(1 << k)
        space[chr(0xF821 + k)] = 1 << k
    (font_dir / 'space.json').write_text(json.dumps({'providers': [{'type': 'space', 'advances': space}]}, ensure_ascii=True), encoding='utf-8')
    # Live text needs ASCII and the small capitals; the rest of nonlatin_european (Cyrillic, Greek...) and the
    # accented capitals would make each of the 236 fonts 16 KB, so they're left out (blank cells).
    keep = set('ᴀʙᴄᴅᴇꜰɢʜɪᴊᴋʟᴍɴᴏᴘǫʀꜱᴛᴜᴠᴡʏᴢ•·★☆✔✘✖❤♦♥♠♣☀☁☂☃✦✧⚡⌚✉✎⚑⚐⚔⛏')
    def trimmed(p):
        if 'nonlatin' not in p.get('file', ''):
            return p
        q = dict(p)
        q['chars'] = [''.join(ch if ch in keep else '\u0000' for ch in row) for row in p['chars']]
        return q
    line_provs = [trimmed(p) for p in provs if 'accented' not in p.get('file', '')]
    for n in range(TOP, BOTTOM + 1):
        shift = n - 6
        out = []
        for p in line_provs:
            if p['type'] == 'space':
                out.append(p)
                continue
            q = dict(p)
            q['ascent'] = p.get('ascent', 7) - shift
            q['height'] = p.get('height', 8)
            out.append(q)
        (font_dir / f'y{n}.json').write_text(json.dumps({'providers': out}, ensure_ascii=False, separators=(',', ':')), encoding='utf-8')
    items_dir = BASE / 'assets' / 'kitui' / 'items'
    models = BASE / 'assets' / 'kitui' / 'models' / 'item'
    textures = BASE / 'assets' / 'kitui' / 'textures' / 'item'
    for d in (items_dir, models, textures):
        d.mkdir(parents=True, exist_ok=True)
    Image.new('RGBA', (16, 16), (0, 0, 0, 0)).save(textures / 'blank.png')
    (models / 'blank.json').write_text(json.dumps({'parent': 'minecraft:item/generated', 'textures': {'layer0': 'kitui:item/blank'}}), encoding='utf-8')
    (items_dir / 'blank.json').write_text(json.dumps({'model': {'type': 'minecraft:model', 'model': 'kitui:item/blank'}}), encoding='utf-8')
    print(f'{len(items)} advances -> {WIDTHS.name}; fonts y{TOP}..y{BOTTOM} + space -> {BASE}')


if __name__ == '__main__':
    main()
