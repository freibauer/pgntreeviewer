#!/usr/bin/env python3
"""Recover transparent piece PNGs from the light/dark-background pairs of
../lichesspuzzleviewer/assets/pieces by difference matting:
  C1 = a*F + (1-a)*B1,  C2 = a*F + (1-a)*B2  =>  a = 1 - (C1-C2)/(B1-B2)
"""
import os, sys
from PIL import Image

SRC = sys.argv[1] if len(sys.argv) > 1 else "../lichesspuzzleviewer/assets/pieces"
DST = sys.argv[2] if len(sys.argv) > 2 else "assets/pieces"
B1 = (240, 217, 181)  # light square
B2 = (181, 136, 99)   # dark square

for color in "wb":
    for p in "KQRBNP":
        l = Image.open(os.path.join(SRC, f"{color}{p}_light.png")).convert("RGB")
        d = Image.open(os.path.join(SRC, f"{color}{p}_dark.png")).convert("RGB")
        out = Image.new("RGBA", l.size)
        for y in range(l.size[1]):
            for x in range(l.size[0]):
                c1, c2 = l.getpixel((x, y)), d.getpixel((x, y))
                a = 1 - sum(c1[i] - c2[i] for i in range(3)) / sum(B1[i] - B2[i] for i in range(3))
                a = max(0.0, min(1.0, a))
                if a < 0.02:
                    out.putpixel((x, y), (0, 0, 0, 0))
                    continue
                f = [round(max(0, min(255, (c1[i] - (1 - a) * B1[i]) / a))) for i in range(3)]
                out.putpixel((x, y), (*f, round(a * 255)))
        out.save(os.path.join(DST, f"{color}{p}.png"))
        print("wrote", f"{color}{p}.png")
