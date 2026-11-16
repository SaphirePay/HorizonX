"""Backpack tier 1: a burlap sack backpack, drawn at 128x128 to sit with tiers 2-6 (light top-left, 1px dark
outline, woven texture, front view, same footprint)."""
import math
import sys
from PIL import Image

W = H = 128
img = Image.new('RGBA', (W, H), (0, 0, 0, 0))
px = img.load()

# Burlap, rope, shadow palettes (light -> dark).
SACK = [(232, 202, 146), (214, 180, 122), (194, 158, 102), (172, 134, 82), (146, 108, 62), (116, 82, 42)]
ROPE = [(214, 176, 112), (178, 138, 80), (138, 100, 54), (100, 70, 36)]
PATCH = [(176, 132, 86), (152, 110, 68), (124, 88, 52)]
LINE = (58, 34, 8, 240)
STITCH = (246, 230, 196)


def put(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        px[x, y] = c if len(c) == 4 else (*c, 255)


def body(x, y):
    """The sack: a rounded box, a little fuller at the bottom, as the other tiers' silhouettes."""
    top, bot = 42, 114
    if y < top or y > bot:
        return False
    t = (y - top) / (bot - top)
    half = 36 + 6 * math.sin(min(1.0, t * 1.6) * math.pi / 2)  # 36 at the neck, 42 lower down
    r = 11
    cx = 64
    # Round the four corners.
    if y < top + r:
        dy = top + r - y
        half = min(half, 30 + math.sqrt(max(0.0, r * r - dy * dy)) - r + 6)
    if y > bot - r:
        dy = y - (bot - r)
        half = half - r + math.sqrt(max(0.0, r * r - dy * dy))
    return abs(x + 0.5 - cx) <= half


def ruffle(x, y):
    """The gathered cloth above the tie: a fan that opens upward."""
    if y < 18 or y > 42:
        return False
    t = (42 - y) / 24
    half = 12 + 13 * t
    # A scalloped top edge: four folds.
    if y < 24:
        fold = (x - 64 + 26) % 13
        edge = 18 + int(3 * abs(math.sin(fold / 13 * math.pi)))
        if y < edge:
            return False
    return abs(x + 0.5 - 64) <= half


def strap(x, y):
    """Rope shoulder straps showing at each side, from under the tie down to the base."""
    for side in (-1, 1):
        if 50 <= y <= 104:
            t = (y - 50) / 54
            sx = 64 + side * (41 + 4 * math.sin(t * math.pi))
            if abs(x + 0.5 - sx) <= 3.2:
                return side
    return 0


# ---- Fill the cloth ----
for y in range(H):
    for x in range(W):
        inb, inr = body(x, y), ruffle(x, y)
        if not (inb or inr):
            continue
        # Light from the top left; edges and base fall off. Ordered dither between steps is the burlap's grain.
        if inb:
            nx = (x - 64) / 42
            ny = (y - 42) / 72
            shade = 1.4 + 0.9 * nx + 1.3 * ny - 0.8 * (1 - nx * nx) * (1 - ny)
            if abs(nx) > 0.84:
                shade += 0.9 if nx > 0 else 0.4
            if ny > 0.88:
                shade += 0.9
        else:
            nx = (x - 64) / 25
            shade = 1.0 + 1.0 * nx + 0.8 * ((y - 18) / 24) - 1.1 * math.sin((x - 40) / 13 * math.pi * 2)
        bayer = ((0, 2), (3, 1))[y % 2][x % 2] / 4 - 0.375
        weave = 0.22 if y % 4 == 0 else 0.0
        i = max(0, min(len(SACK) - 1, int(math.floor(shade + bayer + weave + 0.5))))
        put(x, y, SACK[i])

# ---- A stitched patch on the lower left ----
for y in range(80, 96):
    for x in range(38, 54):
        if body(x, y):
            i = 0 if (x < 40 or y < 82) else (2 if (x > 51 or y > 93) else 1)
            if (x + y) % 4 == 0 and i == 1:
                i = 2
            put(x, y, PATCH[i])
for x in range(38, 54, 3):
    put(x, 79, STITCH)
    put(x + 1, 96, STITCH)
for y in range(80, 96, 3):
    put(37, y, STITCH)
    put(54, y + 1, STITCH)

# ---- Seam down the middle of the belly ----
for y in range(50, 112, 4):
    put(64, y, SACK[4])
    put(64, y + 1, SACK[4])
    put(65, y + 2, SACK[0])

# ---- A stitched seam near the base, like tier 2 ----
for x in range(20, 109):
    if body(x, 102):
        put(x, 102, SACK[5])
        put(x, 103, SACK[4])
        if x % 4 < 2 and body(x, 106):
            put(x, 106, SACK[0])

# ---- The dark mouth of the sack between the folds ----
for x in range(46, 83):
    for y in range(18, 23):
        if ruffle(x, y) and not ruffle(x, y - 1):
            put(x, y, (96, 66, 30))
            if ruffle(x, y + 1):
                put(x, y + 1, (128, 94, 50))

# ---- Rope straps ----
for y in range(H):
    for x in range(W):
        side = strap(x, y)
        if side:
            r = (x + y * 2) % 6
            c = ROPE[0] if r < 2 else (ROPE[1] if r < 4 else ROPE[2])
            put(x, y, c)

# ---- The tie: a twisted rope band around the neck, a knot, two hanging ends ----
for y in range(38, 45):
    for x in range(30, 99):
        if body(x, max(42, y)) or ruffle(x, min(42, y)):
            r = (x - y * 2) % 6
            c = ROPE[0] if r < 2 else (ROPE[1] if r < 4 else ROPE[2])
            if y in (38, 44):
                c = ROPE[3]
            put(x, y, c)
# Knot
for y in range(36, 47):
    for x in range(58, 71):
        d = math.hypot(x - 64, y - 41)
        if d <= 6.8:
            c = ROPE[0] if (x < 63 and y < 41) else (ROPE[1] if d < 5 else ROPE[2])
            put(x, y, c)
# Hanging ends, frayed
for side, x0 in ((-1, 61), (1, 67)):
    for k in range(0, 18):
        x = x0 + side * (k // 5)
        y = 46 + k
        for dx in range(-1, 3):
            put(x + dx, y, (ROPE[0], ROPE[0], ROPE[1], ROPE[2])[dx + 1])
    yb = 64
    for dx in (-2, 0, 2):
        put(x0 + side * 3 + dx, yb, ROPE[0])
        put(x0 + side * 3 + dx, yb + 1, ROPE[1])

# ---- Outline: one dark pixel around everything drawn ----
solid = [[px[x, y][3] > 0 for x in range(W)] for y in range(H)]
for y in range(H):
    for x in range(W):
        if solid[y][x]:
            continue
        if any(0 <= x + dx < W and 0 <= y + dy < H and solid[y + dy][x + dx] for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
            put(x, y, LINE)

out = sys.argv[1] if len(sys.argv) > 1 else 'backpack_tier1_new.png'
img.save(out)
print('saved', out, len(set(img.getdata())), 'colours', img.getbbox())
