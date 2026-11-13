#!/usr/bin/env python3
"""Render a terminal capture (tmux capture-pane -e -p) to a PNG.

Usage: scripts/ansi-to-png.py CAPTURE.ansi OUT.png [CELL_W CELL_H]

Draws each cell's background, then its glyph in its foreground colour.
The block characters tetron-browser paints pages with (half and quadrant
blocks) are drawn as exact rectangles, so the page looks as it does in a
terminal; other text uses DejaVu Sans Mono. Needs Pillow.
"""
import re
import sys

from PIL import Image, ImageDraw, ImageFont

FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
DEF_FG, DEF_BG = (220, 223, 228), (14, 17, 22)
BASE16 = [(0, 0, 0), (205, 49, 49), (13, 188, 121), (229, 229, 16), (36, 114, 200), (188, 63, 188), (17, 168, 205), (229, 229, 229),
          (102, 102, 102), (241, 76, 76), (35, 209, 139), (245, 245, 67), (59, 142, 234), (214, 112, 214), (41, 184, 219), (255, 255, 255)]


def xterm256(n):
    if n < 16:
        return BASE16[n]
    if n < 232:
        n -= 16
        v = [0, 95, 135, 175, 215, 255]
        return (v[n // 36], v[n // 6 % 6], v[n % 6])
    g = 8 + (n - 232) * 10
    return (g, g, g)


# Block glyphs as filled boxes in eighths of a cell: (x0, y0, x1, y1).
BLOCKS = {
    "█": [(0, 0, 8, 8)], "▀": [(0, 0, 8, 4)], "▄": [(0, 4, 8, 8)], "▌": [(0, 0, 4, 8)], "▐": [(4, 0, 8, 8)],
    "▖": [(0, 4, 4, 8)], "▗": [(4, 4, 8, 8)], "▘": [(0, 0, 4, 4)], "▝": [(4, 0, 8, 4)],
    "▙": [(0, 0, 4, 8), (4, 4, 8, 8)], "▛": [(0, 0, 8, 4), (0, 4, 4, 8)], "▜": [(0, 0, 8, 4), (4, 4, 8, 8)],
    "▟": [(4, 0, 8, 8), (0, 4, 4, 8)], "▚": [(0, 0, 4, 4), (4, 4, 8, 8)], "▞": [(4, 0, 8, 4), (0, 4, 4, 8)],
}


def parse(text):
    rows = []
    for line in text.split("\n"):
        fg, bg, row = DEF_FG, DEF_BG, []
        for tok in re.split(r"(\x1b\[[0-9;:]*m)", line):
            if tok.startswith("\x1b["):
                p = [int(x) if x else 0 for x in re.split("[;:]", tok[2:-1])] or [0]
                i = 0
                while i < len(p):
                    c = p[i]
                    if c == 0:
                        fg, bg = DEF_FG, DEF_BG
                    elif c in (38, 48) and i + 1 < len(p):
                        if p[i + 1] == 2 and i + 4 < len(p):
                            col = tuple(p[i + 2:i + 5]); i += 4
                        elif p[i + 1] == 5 and i + 2 < len(p):
                            col = xterm256(p[i + 2]); i += 2
                        else:
                            col = None
                        if col:
                            fg, bg = (col, bg) if c == 38 else (fg, col)
                    elif c == 39:
                        fg = DEF_FG
                    elif c == 49:
                        bg = DEF_BG
                    elif 30 <= c <= 37:
                        fg = BASE16[c - 30]
                    elif 90 <= c <= 97:
                        fg = BASE16[c - 82]
                    elif 40 <= c <= 47:
                        bg = BASE16[c - 40]
                    elif 100 <= c <= 107:
                        bg = BASE16[c - 92]
                    i += 1
            else:
                row.extend((ch, fg, bg) for ch in tok)
        rows.append(row)
    while rows and not rows[-1]:
        rows.pop()
    return rows


def main():
    src, out = sys.argv[1], sys.argv[2]
    cw, ch = (int(sys.argv[3]), int(sys.argv[4])) if len(sys.argv) > 4 else (10, 20)
    rows = parse(open(src, encoding="utf-8").read())
    cols = max(len(r) for r in rows)
    img = Image.new("RGB", (cols * cw, len(rows) * ch), DEF_BG)
    d = ImageDraw.Draw(img)
    font = ImageFont.truetype(FONT, int(ch * 0.8))
    for y, row in enumerate(rows):
        for x, (c, fg, bg) in enumerate(row):
            x0, y0 = x * cw, y * ch
            d.rectangle([x0, y0, x0 + cw - 1, y0 + ch - 1], fill=bg)
            if c in BLOCKS:
                for a, b, e, f in BLOCKS[c]:
                    d.rectangle([x0 + a * cw // 8, y0 + b * ch // 8, x0 + e * cw // 8 - 1, y0 + f * ch // 8 - 1], fill=fg)
            elif c.strip():
                d.text((x0, y0 + ch * 0.05), c, font=font, fill=fg)
    img.save(out, optimize=True)


if __name__ == "__main__":
    main()
