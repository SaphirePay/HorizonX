#!/usr/bin/env python3
"""Drawing kit for menus drawn by a resource pack (horizon_kit::ui): pixel-exact text in the game's font, the game's
item and block textures, panels, bars, and three finished window styles:

    A  Vanilla+   the game's window language in purple: stepped outline, bevels, sunken slots, its button texture
    B  Midnight   flat and modern: dark rounded cards, letter-spaced small capitals, one accent colour
    C  Crafted    a block texture (planks, deepslate...) darkened, gold trim, framed panels

    import uidraw as U
    c = U.Canvas(rows=6, left=0, right=0, top=0)       # room past the window: side panels, a banner on top
    U.a_window(c, 'Bank')                              # or U.b_window(c, accent) / U.c_window(c, 'block/dark_oak_planks')
    U.text_in(c, U.span(0, 0, 1, 8), 12, 22, 'Balance', U.A_LABEL)
    U.a_button(c, U.span(5, 2, 5, 6, 1), 'Deposit', (40, 128, 66, 255))
    U.blanks(c, 5, 2, 5, 6, 'Deposit')                 # the clickable area the plugin will put there
    U.save(c, 'mymenus/art', 'bank')                   # art/bank.png + bank.json for tools/uipack.py

check(c, name) reports text under an item, text over text, and text out of its box; save() refuses a canvas with
problems. Window maths: slot (r, c) has its frame at (7 + 18c, 17 + 18r); the player's inventory label is at
rows*18 + 20, its slots from rows*18 + 31, the hotbar at rows*18 + 89. Live text (names, numbers) is not drawn
here: leave room and place it from the plugin with ui::Title.

Set HORIZON_ASSETS to the game's assets/minecraft folder if it isn't the launcher's default.
"""
import json
import os
import pathlib

from PIL import Image, ImageDraw

GAME = pathlib.Path(os.environ.get('HORIZON_ASSETS', r'%USERPROFILE%\AppData\Roaming\horizon-launcher\server-runtime\game-files\horizon-client\26.3\assets\minecraft'))
TEX = GAME / 'textures'
_FONT = None


def font():
    """The default font's glyphs, in the game's order of providers, each 8 px tall."""
    global _FONT
    if _FONT is None:
        _FONT = {}
        default = json.loads((GAME / 'font' / 'include' / 'default.json').read_text(encoding='utf-8'))
        for p in default['providers']:
            if p['type'] != 'bitmap' or p.get('height', 8) != 8:
                continue
            sheet = Image.open(TEX / p['file'].split(':')[-1]).convert('RGBA')
            rows = p['chars']
            cw, chh = sheet.width // len(rows[0]), sheet.height // len(rows)
            for r, row in enumerate(rows):
                for c, ch in enumerate(row):
                    if ch not in _FONT and ch != '\u0000':
                        _FONT[ch] = sheet.crop((c * cw, r * chh, (c + 1) * cw, (r + 1) * chh))
    return _FONT


def glyph_width(g):
    for x in range(g.width - 1, -1, -1):
        for y in range(g.height):
            if g.getpixel((x, y))[3]:
                return x + 1
    return 0


_BTN = None


def button_sprite(w, h, fill):
    """The game's button texture, nine-sliced to w x h, its greys turned into shades of `fill`."""
    global _BTN
    if _BTN is None:
        _BTN = Image.open(TEX / 'gui' / 'sprites' / 'widget' / 'button.png').convert('RGBA')
    src, b = _BTN, 3
    out = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    sw, sh = src.size

    def tile(sx0, sy0, sx1, sy1, dx0, dy0, dx1, dy1):
        piece = src.crop((sx0, sy0, sx1, sy1))
        for yy in range(dy0, dy1, piece.height):
            for xx in range(dx0, dx1, piece.width):
                out.paste(piece.crop((0, 0, min(piece.width, dx1 - xx), min(piece.height, dy1 - yy))), (xx, yy))
    tile(0, 0, b, b, 0, 0, b, b)
    tile(sw - b, 0, sw, b, w - b, 0, w, b)
    tile(0, sh - b, b, sh, 0, h - b, b, h)
    tile(sw - b, sh - b, sw, sh, w - b, h - b, w, h)
    tile(b, 0, sw - b, b, b, 0, w - b, b)
    tile(b, sh - b, sw - b, sh, b, h - b, w - b, h)
    tile(0, b, b, sh - b, 0, b, b, h - b)
    tile(sw - b, b, sw, sh - b, w - b, b, w, h - b)
    tile(b, b, sw - b, sh - b, b, b, w - b, h - b)
    px = out.load()
    for yy in range(h):
        for xx in range(w):
            r, g, bl, a = px[xx, yy]
            if a:
                px[xx, yy] = (0, 0, 0, 255) if r < 8 else tuple(max(0, min(255, int(v * r / 0x6f))) for v in fill[:3]) + (255,)
    return out


# ---------------------------------------------------------------- canvas


class Canvas:
    """A picture for a window of `rows` rows, with room past its edges: left, right, top. Draw in window
    coordinates (0, 0 = the window's top-left corner)."""

    def __init__(self, rows, left=0, right=0, top=0):
        self.rows, self.L, self.R, self.T = rows, left, right, top
        self.H = 114 + rows * 18
        self.im = Image.new('RGBA', (left + 176 + right, top + self.H), (0, 0, 0, 0))
        self.d = ImageDraw.Draw(self.im)
        self.ink = {}
        self.call = 0
        self.problems = []
        self.slots = []

    def xy(self, x, y):
        return x + self.L, y + self.T

    def box(self, b):
        x0, y0, x1, y1 = b
        return [x0 + self.L, y0 + self.T, x1 + self.L, y1 + self.T]

    def rect(self, b, fill=None, outline=None):
        self.d.rectangle(self.box(b), fill=fill, outline=outline)

    def line(self, b, fill):
        self.d.line(self.box(b), fill=fill)

    def point(self, x, y, fill):
        self.d.point(self.xy(x, y), fill=fill)

    def clear(self, x, y):
        self.im.putpixel(self.xy(x, y), (0, 0, 0, 0))

    def paste(self, img, x, y):
        self.im.alpha_composite(img, self.xy(x, y))

    def put_ink(self, x, y, colour):
        X, Y = self.xy(x, y)
        if not (0 <= X < self.im.width and 0 <= Y < self.im.height):
            self.problems.append(f'text off the picture at {(x, y)}')
            return
        self.im.putpixel((X, Y), colour)
        was = self.ink.get((x, y))
        if was is not None and was != self.call:
            self.problems.append(f'text over text at {(x, y)}')
        self.ink[(x, y)] = self.call


def cell(r, c):
    return 7 + 18 * c, 17 + 18 * r


def span(r0, c0, r1, c1, inset=0):
    return (7 + 18 * c0 + inset, 17 + 18 * r0 + inset, 7 + 18 * c1 + 17 - inset, 17 + 18 * r1 + 17 - inset)


def S(r, c):
    return r * 9 + c


# ---------------------------------------------------------------- text

SMALL = dict(zip('abcdefghijklmnopqrstuvwxyz', 'ᴀʙᴄᴅᴇꜰɢʜɪᴊᴋʟᴍɴᴏᴘqʀꜱᴛᴜᴠᴡxʏᴢ'))


def sc(s):
    """Small capitals, the way the server writes labels."""
    return ''.join(SMALL.get(ch, ch) for ch in s.lower())


def _glyph(ch):
    f = font()
    return f.get(ch) or f['?']


def _gw(ch):
    return 3 if ch == ' ' else glyph_width(_glyph(ch))


def measure(s, scale=1, bold=False, track=0):
    if not s:
        return 0
    w = 0
    for ch in s:
        w += (_gw(ch) + 1 + (1 if bold and ch != ' ' else 0) + track) if ch != ' ' else (4 + track)
    return (w - 1 - track) * scale


def text(c, x, y, s, colour, scale=1, shadow=False, bold=False, track=0):
    """`s` at (x, y) (top of the 8 px cell), `scale` times as big; returns where it ends."""
    c.call += 1
    if shadow:
        sh = shadow if isinstance(shadow, tuple) else tuple(int(v * 0.25) for v in colour[:3]) + (255,)
        _draw(c, x + scale, y + scale, s, sh, scale, bold, track)
    return _draw(c, x, y, s, colour, scale, bold, track)


def _draw(c, x, y, s, colour, scale, bold, track):
    for ch in s:
        if ch == ' ':
            x += (4 + track) * scale
            continue
        g = _glyph(ch)
        w = glyph_width(g)
        for gx in range(w + (1 if bold else 0)):
            for gy in range(g.height):
                on = (gx < w and g.getpixel((gx, gy))[3]) or (bold and gx > 0 and gx - 1 < w and g.getpixel((gx - 1, gy))[3])
                if on:
                    for sx in range(scale):
                        for sy in range(scale):
                            c.put_ink(x + gx * scale + sx, y + gy * scale + sy, colour)
        x += (w + 1 + (1 if bold else 0) + track) * scale
    return x


def centre(c, cx, y, s, colour, **k):
    w = measure(s, k.get('scale', 1), k.get('bold', False), k.get('track', 0))
    return text(c, cx - w // 2, y, s, colour, **k)


def right(c, rx, y, s, colour, **k):
    w = measure(s, k.get('scale', 1), k.get('bold', False), k.get('track', 0))
    return text(c, rx - w, y, s, colour, **k)


def fits(c, box, x, w, what):
    if x < box[0] + 3 or x + w > box[2] - 2:
        c.problems.append(f'"{what}" ({x}..{x + w}) runs out of its box {box[0]}..{box[2]}')


def text_in(c, box, x, y, s, colour, **k):
    w = measure(s, k.get('scale', 1), k.get('bold', False), k.get('track', 0))
    fits(c, box, x, w, s)
    return text(c, x, y, s, colour, **k)


def right_in(c, box, y, s, colour, pad=5, **k):
    w = measure(s, k.get('scale', 1), k.get('bold', False), k.get('track', 0))
    x = box[2] - pad - w
    fits(c, box, x, w, s)
    return text(c, x, y, s, colour, **k)


def centre_in(c, box, y, s, colour, **k):
    w = measure(s, k.get('scale', 1), k.get('bold', False), k.get('track', 0))
    x = (box[0] + box[2] + 1) // 2 - w // 2
    fits(c, box, x, w, s)
    return text(c, x, y, s, colour, **k)


# ---------------------------------------------------------------- art

_TEX = {}


def tex(path):
    """A texture of the game (`item/bow`, `block/dark_oak_planks`), its first frame."""
    if path not in _TEX:
        f = TEX / f'{path}.png'
        if not f.exists():
            raise SystemExit(f'no texture {path}')
        im = Image.open(f).convert('RGBA')
        _TEX[path] = im.crop((0, 0, im.width, im.width))
    return _TEX[path]


def art(c, path, x, y, scale=1, dim=1.0):
    im = tex(path)
    if im.width != 16:
        im = im.resize((16, 16), Image.NEAREST)
    if scale != 1:
        im = im.resize((16 * scale, 16 * scale), Image.NEAREST)
    if dim != 1.0:
        im = shade_img(im, dim)
    c.paste(im, x, y)


def shade(col, f):
    return tuple(max(0, min(255, int(v * f))) for v in col[:3]) + (col[3] if len(col) > 3 else 255,)


def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3)) + (255,)


def shade_img(im, f, tint=None, t=0.0):
    px = im.copy()
    p = px.load()
    for yy in range(px.height):
        for xx in range(px.width):
            r, g, b, a = p[xx, yy]
            col = shade((r, g, b), f)
            if tint:
                col = mix(col, tint, t)
            p[xx, yy] = col[:3] + (a,)
    return px


def tiled(c, b, path, f=0.45, tint=None, t=0.0):
    """Fill box b with a block texture, darkened by f (and tinted), tiled at 16 px."""
    x0, y0, x1, y1 = b
    tile = shade_img(tex(path).resize((16, 16), Image.NEAREST), f, tint, t)
    for yy in range(y0, y1 + 1, 16):
        for xx in range(x0, x1 + 1, 16):
            w, h = min(16, x1 + 1 - xx), min(16, y1 + 1 - yy)
            c.paste(tile.crop((0, 0, w, h)), xx, yy)


def rounded(c, b, fill, outline=None, r=1):
    x0, y0, x1, y1 = b
    c.rect((x0 + r, y0, x1 - r, y1), fill=fill)
    c.rect((x0, y0 + r, x1, y1 - r), fill=fill)
    if r == 2:
        c.point(x0 + 1, y0 + 1, fill)
        c.point(x1 - 1, y0 + 1, fill)
        c.point(x0 + 1, y1 - 1, fill)
        c.point(x1 - 1, y1 - 1, fill)
    if outline:
        c.line((x0 + r, y0, x1 - r, y0), outline)
        c.line((x0 + r, y1, x1 - r, y1), outline)
        c.line((x0, y0 + r, x0, y1 - r), outline)
        c.line((x1, y0 + r, x1, y1 - r), outline)
        if r == 2:
            for (x, y) in ((x0 + 1, y0 + 1), (x1 - 1, y0 + 1), (x0 + 1, y1 - 1), (x1 - 1, y1 - 1)):
                c.point(x, y, outline)


def bar(c, b, frac, fill, bg, hi=None):
    x0, y0, x1, y1 = b
    c.rect(b, fill=bg)
    if frac > 0:
        xe = x0 + max(1, int((x1 - x0 + 1) * min(1.0, frac))) - 1
        c.rect((x0, y0, xe, y1), fill=fill)
        if hi and y1 > y0:
            c.line((x0, y0, xe, y0), hi)


def item(c, slot, mat, name, lore=(), model=None, glint=False, action=None, count=1):
    c.slots.append({'slot': slot, 'item': mat, 'name': name, 'lore': list(lore), 'model': model, 'glint': glint,
                    'count': count, 'action': action, 'hide': False})


def blanks(c, r0, c0, r1, c1, name, lore=(), action=None):
    for r in range(r0, r1 + 1):
        for cc in range(c0, c1 + 1):
            item(c, S(r, cc), 'PAPER', name, lore, 'kitui:blank', action=action)


# ---------------------------------------------------------------- palettes and the three windows

WHITE = (244, 244, 245, 255)
GOLD = (253, 224, 71, 255)
GOLD_D = (161, 98, 7, 255)
GREEN = (74, 222, 128, 255)
GREEN_D = (22, 101, 52, 255)
RED = (248, 113, 113, 255)
RED_D = (127, 29, 29, 255)
CYAN = (103, 232, 249, 255)
PINK = (244, 114, 182, 255)
ORANGE = (251, 146, 60, 255)
VIOLET = (167, 139, 250, 255)
SILVER = (203, 213, 225, 255)
BRONZE = (217, 119, 6, 255)

# A: the game's window, purple
A_OUT = (6, 3, 12, 255)
A_SHADOW = (26, 17, 46, 255)
A_BODY = (54, 39, 90, 255)
A_LIGHT = (104, 82, 158, 255)
A_SLOT_D = (18, 11, 32, 255)
A_SLOT = (36, 25, 62, 255)
A_SLOT_L = (110, 88, 166, 255)
A_LABEL = (226, 218, 248, 255)
A_SOFT = (168, 152, 212, 255)

# B: midnight
B_BG = (16, 13, 24, 255)
B_CARD = (26, 22, 38, 255)
B_CARD_HI = (36, 30, 52, 255)
B_LINE = (46, 39, 68, 255)
B_TEXT = (241, 238, 250, 255)
B_SUB = (139, 128, 172, 255)
B_FAINT = (84, 75, 112, 255)

# C: crafted
C_TRIM = (201, 162, 39, 255)
C_TRIM_D = (110, 80, 18, 255)
C_TEXT = (255, 247, 225, 255)
C_SUB = (214, 196, 160, 255)


def a_sunken(c, b, fill=None):
    x0, y0, x1, y1 = b
    c.rect(b, fill=fill or A_SLOT)
    c.line((x0, y0, x1 - 1, y0), A_SLOT_D)
    c.line((x0, y0, x0, y1 - 1), A_SLOT_D)
    c.line((x0 + 1, y1, x1, y1), A_SLOT_L)
    c.line((x1, y0 + 1, x1, y1), A_SLOT_L)


def a_well(c, r, cc, fill=None):
    x, y = cell(r, cc)
    a_sunken(c, (x, y, x + 17, y + 17), fill)


def a_raised(c, b, fill, light=None, dark=None):
    x0, y0, x1, y1 = b
    c.rect(b, fill=fill)
    c.rect(b, outline=A_OUT)
    c.line((x0 + 1, y0 + 1, x1 - 2, y0 + 1), light or shade(fill, 1.55))
    c.line((x0 + 1, y0 + 1, x0 + 1, y1 - 2), light or shade(fill, 1.55))
    c.line((x0 + 2, y1 - 1, x1 - 1, y1 - 1), dark or shade(fill, 0.55))
    c.line((x1 - 1, y0 + 2, x1 - 1, y1 - 1), dark or shade(fill, 0.55))
    for (x, y) in ((x0, y0), (x1, y0), (x0, y1), (x1, y1)):
        c.clear(x, y)


def a_button(c, b, label, fill=(78, 58, 124, 255), colour=WHITE):
    x0, y0, x1, y1 = b
    sprite = button_sprite(x1 - x0 + 1, y1 - y0 + 1, fill)
    c.paste(sprite, x0, y0)
    centre_in(c, b, (y0 + y1) // 2 - 4, label, colour, shadow=True)


def a_window(c, title=None, inv=True):
    """The game's window outline and bevels at 176 x H, the player's inventory, the title."""
    W, H = 176, c.H
    c.rect((1, 1, W - 2, H - 2), fill=A_BODY)
    c.line((2, 0, W - 4, 0), A_OUT)
    c.line((0, 2, 0, H - 4), A_OUT)
    c.line((3, H - 1, W - 3, H - 1), A_OUT)
    c.line((W - 1, 3, W - 1, H - 3), A_OUT)
    for (x, y) in ((1, 1), (W - 3, 1), (W - 2, 2), (1, H - 3), (2, H - 2), (W - 2, H - 2)):
        c.point(x, y, A_OUT)
    for (x, y) in ((0, 0), (1, 0), (0, 1), (W - 3, 0), (W - 2, 0), (W - 1, 0), (W - 2, 1), (W - 1, 1), (W - 1, 2),
                   (0, H - 3), (0, H - 2), (0, H - 1), (1, H - 2), (1, H - 1), (2, H - 1), (W - 2, H - 1), (W - 1, H - 1), (W - 1, H - 2)):
        c.clear(x, y)
    c.rect((2, 1, W - 4, 2), fill=A_LIGHT)
    c.rect((1, 2, 2, H - 4), fill=A_LIGHT)
    c.point(3, 3, A_LIGHT)
    c.rect((3, H - 3, W - 3, H - 2), fill=A_SHADOW)
    c.rect((W - 3, 3, W - 2, H - 3), fill=A_SHADOW)
    c.point(W - 4, H - 4, A_SHADOW)
    if title:
        text(c, 8, 6, title, A_LABEL)
    if inv:
        base_y = c.rows * 18
        text(c, 8, base_y + 20, 'Inventory', A_LABEL)
        for r in range(3):
            for cc in range(9):
                x, y = 7 + cc * 18, base_y + 31 + r * 18
                a_sunken(c, (x, y, x + 17, y + 17))
        for cc in range(9):
            x, y = 7 + cc * 18, base_y + 89
            a_sunken(c, (x, y, x + 17, y + 17))


def b_slot(c, x, y, fill=None, edge=None):
    rounded(c, (x, y, x + 17, y + 17), fill or B_CARD_HI, edge)


def b_window(c, accent):
    W, H = 176, c.H
    rounded(c, (0, 0, W - 1, H - 1), B_BG, B_LINE, r=2)
    base_y = c.rows * 18
    end = text(c, 8, base_y + 20, sc('inventory'), B_SUB, track=1)
    c.line((end + 4, base_y + 24, W - 8, base_y + 24), B_LINE)
    for r in range(3):
        for cc in range(9):
            b_slot(c, 7 + cc * 18, base_y + 31 + r * 18, B_CARD, B_LINE)
    for cc in range(9):
        b_slot(c, 7 + cc * 18, base_y + 89, B_CARD, B_LINE)


def b_button(c, b, label, fill, colour=WHITE, edge=None):
    rounded(c, b, fill, edge or shade(fill, 1.4), r=2)
    c.line((b[0] + 2, b[3] - 1, b[2] - 2, b[3] - 1), shade(fill, 0.7))
    centre_in(c, b, (b[1] + b[3]) // 2 - 4, sc(label), colour, track=1)


def c_frame(c, b, inner=None):
    """Gold trim: a dark line, a gold line, a dark line."""
    x0, y0, x1, y1 = b
    c.rect(b, outline=(20, 14, 6, 255))
    c.rect((x0 + 1, y0 + 1, x1 - 1, y1 - 1), outline=C_TRIM)
    c.line((x0 + 2, y1 - 1, x1 - 1, y1 - 1), C_TRIM_D)
    c.line((x1 - 1, y0 + 2, x1 - 1, y1 - 1), C_TRIM_D)
    c.rect((x0 + 2, y0 + 2, x1 - 2, y1 - 2), outline=(20, 14, 6, 255))
    if inner:
        c.rect((x0 + 3, y0 + 3, x1 - 3, y1 - 3), fill=inner)


def c_slot(c, x, y, fill=(14, 10, 6, 200)):
    c.rect((x, y, x + 17, y + 17), fill=fill)
    c.line((x, y, x + 17, y), (0, 0, 0, 255))
    c.line((x, y, x, y + 17), (0, 0, 0, 255))
    c.line((x + 1, y + 17, x + 17, y + 17), (120, 92, 52, 255))
    c.line((x + 17, y + 1, x + 17, y + 17), (120, 92, 52, 255))


def c_window(c, block, f=0.42, tint=None, t=0.0):
    W, H = 176, c.H
    tiled(c, (0, 0, W - 1, H - 1), block, f, tint, t)
    c_frame(c, (0, 0, W - 1, H - 1))
    base_y = c.rows * 18
    text(c, 8, base_y + 20, 'Inventory', C_SUB, shadow=True)
    for r in range(3):
        for cc in range(9):
            c_slot(c, 7 + cc * 18, base_y + 31 + r * 18)
    for cc in range(9):
        c_slot(c, 7 + cc * 18, base_y + 89)


def c_button(c, b, label, fill):
    x0, y0, x1, y1 = b
    sprite = button_sprite(x1 - x0 + 1, y1 - y0 + 1, fill)
    c.paste(sprite, x0, y0)
    centre_in(c, b, (y0 + y1) // 2 - 4, label, WHITE, shadow=True)


def plate(c, b, fill, light, dark, outline=(0, 0, 0, 255)):
    x0, y0, x1, y1 = b
    c.rect(b, fill=fill)
    c.rect(b, outline=outline)
    c.line((x0 + 1, y0 + 1, x1 - 1, y0 + 1), light)
    c.line((x0 + 1, y1 - 1, x1 - 1, y1 - 1), dark)


def chip(c, x, y, label, fill, edge, colour=WHITE):
    w = measure(label) + 7
    rounded(c, (x, y, x + w, y + 9), fill, edge)
    text(c, x + 4, y + 1, label, colour)
    return x + w




def check(c, name=''):
    """Text under an item, text over text, text out of its box: a list of problems (empty: fine)."""
    for s in c.slots:
        if s.get('model') in ('kitui:blank', 'mock:blank'):
            continue
        r, cc = divmod(s['slot'], 9)
        x0, y0 = 8 + 18 * cc, 18 + 18 * r
        if any(x0 <= x <= x0 + 15 and y0 <= y <= y0 + 15 for (x, y) in c.ink):
            c.problems.append(f'text under the item in slot {s["slot"]}')
    return [f'{name}: {p}' for p in dict.fromkeys(c.problems)]


def save(c, folder, name):
    """<folder>/<name>.png and <name>.json for tools/uipack.py, and <name>.slots.json (what goes in which slot)."""
    problems = check(c, name)
    if problems:
        raise SystemExit('\n'.join(problems[:20]))
    folder = pathlib.Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    c.im.save(folder / f'{name}.png')
    (folder / f'{name}.json').write_text(json.dumps({'left': c.L, 'top': c.T, 'rows': c.rows}), encoding='utf-8')
    (folder / f'{name}.slots.json').write_text(json.dumps(c.slots, ensure_ascii=False, indent=1), encoding='utf-8')
