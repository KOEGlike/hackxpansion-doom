#!/usr/bin/env python3
"""Builds the E1M1-only WAD subset for the hackxpansion doom app.

Usage: python3 tools/make_stripped_wad.py <doom1.wad> <out.wad> [report.json]

Run from doom-core/ (reads c/info.c for the thing->sprite mapping). The output
keeps only the lumps the vanilla engine needs to boot into and play E1M1 with
all E1M1 things: map data, palettes, textures/patches, flats, sprites, status
bar graphics, menu graphics, the intermission screen and the intro demos.
Sounds, music, the remaining eight levels and splash screens are dropped.
"""
import json
import re
import struct
import sys
from collections import defaultdict

src = open(sys.argv[1], "rb").read()
magic, numlumps, infotableofs = struct.unpack("<4sii", src[:12])
assert magic == b"IWAD", magic
lumps = []
for i in range(numlumps):
    off, size = struct.unpack("<ii", src[infotableofs + i * 16 : infotableofs + i * 16 + 8])
    name = src[infotableofs + i * 16 + 8 : infotableofs + i * 16 + 16].rstrip(b"\0").decode()
    lumps.append((name, off, size))
by = {n: (o, s) for n, o, s in lumps}
names = {n for n, _, _ in lumps}

# --- sprite prefixes needed by E1M1 things (parsed from info.c) --------------
info = open("c/info.c").read()
sprnames = re.findall(
    r'"([^"]+)"',
    re.search(r"char \*sprnames\[\] = \{(.*?)\};", info, re.S).group(1),
)
name_to_spr = {}
for mm in re.finditer(r"\{(SPR_\w+)[^;]*?\}\s*,\s*//\s*(S_\w+)", info):
    name_to_spr[mm.group(2)] = mm.group(1)
mo, cur = [], None
for line in info.splitlines():
    c = re.search(r"//\s*(MT_\w+)", line)
    if c:
        cur = [c.group(1), None]
    if cur:
        dn = re.match(r"\s*(-?\d+),\s*//\s*doomednum", line)
        if dn:
            cur[1] = int(dn.group(1))
        ss = re.match(r"\s*(S_\w+),\s*//\s*spawnstate", line)
        if ss:
            cur.append(ss.group(1))
            mo.append(cur)
            cur = None
doomed = {n: name_to_spr[st][4:] for name, n, st in mo if n >= 0 and st in name_to_spr}

e1m1 = next(i for i, (n, _, _) in enumerate(lumps) if n == "E1M1")
e1m2 = next(i for i, (n, _, _) in enumerate(lumps) if n == "E1M2")
things = lumps[e1m1 + 1]
types = set()
off = 0
while off < things[2]:
    x, y, a, t, o = struct.unpack("<5h", src[things[1] + off : things[1] + off + 10])
    off += 10
    if t > 0:
        types.add(t)
sprites = {doomed[t] for t in types if t in doomed}
sprites.update(
    ["PLAY", "PUFF", "BEXP", "BLUD", "TFOG", "IFOG", "ARM1", "ARM2",
     "PUNG", "PISG", "PISF", "SHTG", "SHTF", "CHGG", "CHGF", "MISG", "MISF",
     "SAWG", "SAWF", "AMMO", "SBOX", "CLIP", "STIM", "MEDI", "BON1", "BON2"]
)

# --- E1M1 textures (and their patches) + flats -------------------------------
pn_off, _ = by["PNAMES"]
npat = struct.unpack("<i", src[pn_off : pn_off + 4])[0]
pn = [src[pn_off + 4 + i * 8 : pn_off + 4 + (i + 1) * 8].rstrip(b"\0").decode() for i in range(npat)]
to, _ = by["TEXTURE1"]
ntex = struct.unpack("<i", src[to : to + 4])[0]
toffs = struct.unpack("<%di" % ntex, src[to + 4 : to + 4 + 4 * ntex])
tp = defaultdict(list)
for i, off in enumerate(toffs):
    o = to + off
    tn = src[o : o + 8].rstrip(b"\0").decode()
    pc = struct.unpack("<h", src[o + 20 : o + 22])[0]
    for p in range(pc):
        po = o + 22 + p * 10
        tp[tn].append(pn[struct.unpack("<hhh", src[po : po + 6])[2]])

# map lumps by directory index: E1M1, THINGS, LINEDEFS, SIDEDEFS, VERTEXES,
# SEGS, SSECTORS, NODES, SECTORS, REJECT, BLOCKMAP
sidedefs = lumps[e1m1 + 3]
sectors = lumps[e1m1 + 8]
sd, fl = set(), set()
for i in range(sidedefs[2] // 30):
    o = sidedefs[1] + i * 30
    for t in (src[o + 4 : o + 12], src[o + 12 : o + 20], src[o + 20 : o + 28]):
        nm = t.rstrip(b"\0").decode()
        if nm and nm != "-":
            sd.add(nm)
for i in range(sectors[2] // 26):
    o = sectors[1] + i * 26
    for f in (src[o + 4 : o + 12], src[o + 12 : o + 20]):
        nm = f.rstrip(b"\0").decode()
        if nm:
            fl.add(nm)
tex = {t for t in sd if t in tp}
pat = set()
for t in tex:
    pat.update(tp[t])
fl.add("FLOOR4_8")  # E1 finale background

# --- keep set ---------------------------------------------------------------
marker_names = {
    "S_START", "S_END", "P_START", "P_END", "F_START", "F_END",
    "P1_START", "P1_END", "P2_START", "P2_END", "P3_START", "P3_END",
    "F1_START", "F1_END", "F2_START", "F2_END",
}
keepidx = set(range(e1m1, e1m2))  # E1M1 block, incl. the zero-size marker
keep = {"PLAYPAL", "COLORMAP", "TEXTURE1", "PNAMES", "DEMO1", "DEMO2", "DEMO3"}
keep.update({l[0] for l in lumps if l[0][:2] in ("M_", "ST", "WI", "BR", "AM")})
keep.update(pat)
keep.update({f for f in fl if f in names})
keep.update({l[0] for l in lumps if l[0][:4] in sprites and l[0][:4] not in ("S_START", "S_END")})

missing_sprites = sprites - {l[0][:4] for l in lumps}
missing_patches = pat - names
missing_flats = {f for f in fl if f not in names}
for label, m in (("sprite", missing_sprites), ("patch", missing_patches), ("flat", missing_flats)):
    if m:
        print("MISSING", label, m)

report = defaultdict(lambda: [0, 0])
for i, (n, o, s) in enumerate(lumps):
    if s > 0 and (i in keepidx or n in keep):
        if n in ("PLAYPAL", "COLORMAP", "TEXTURE1", "PNAMES"):
            k = n
        elif n[:2] == "M_":
            k = "menus"
        elif n[:2] == "ST":
            k = "status+font"
        elif n[:2] == "WI":
            k = "intermission"
        elif n[:2] == "BR" or n[:2] == "AM":
            k = "border/automap"
        elif n in pat:
            k = "patches"
        elif n in fl:
            k = "flats"
        elif n[:4] in sprites:
            k = "sprites"
        else:
            k = "???"
        report[k][0] += 1
        report[k][1] += s
for k in sorted(report, key=lambda k: -report[k][1]):
    print(f"{k:14s} n={report[k][0]:4d} bytes={report[k][1]:9d}")
print("TOTAL", sum(r[1] for r in report.values()))

kept = [
    (n, off, sz)
    for i, (n, off, sz) in enumerate(lumps)
    if i in keepidx or n in keep or n in marker_names
]
print(f"keep lumps: {len(kept)}  bytes: {sum(e[2] for e in kept)}")

entries = b""
newbody = b""
dataofs = 12 + 16 * len(kept)
for n, off, sz in kept:
    entries += struct.pack("<ii", dataofs, sz) + n.encode().ljust(8, b"\0")
    newbody += src[off : off + sz]
    dataofs += sz
head = struct.pack("<4sii", b"IWAD", len(kept), 12)
open(sys.argv[2], "wb").write(head + entries + newbody)
print("wrote", sys.argv[2], len(head) + len(entries) + len(newbody))
if len(sys.argv) > 3:
    json.dump(
        {k: dict(zip(("lumps", "bytes"), v)) for k, v in report.items()},
        open(sys.argv[3], "w"),
    )
