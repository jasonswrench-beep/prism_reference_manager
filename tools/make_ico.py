"""Assemble a Windows .ico from PNG files (PNG-compressed entries).
Small sizes use the simplified silhouette; larger sizes use the full icon.
Usage: python make_ico.py out.ico 16.png 20.png ... 256.png"""
import struct, sys

def size_of(png):
    w, h = struct.unpack(">II", png[16:24])
    return w, h

out, files = sys.argv[1], sys.argv[2:]
pngs = [open(f, "rb").read() for f in files]
header = struct.pack("<HHH", 0, 1, len(pngs))
entries, data = b"", b""
offset = 6 + 16 * len(pngs)
for p in pngs:
    w, h = size_of(p)
    entries += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(p), offset + len(data))
    data += p
open(out, "wb").write(header + entries + data)
print("wrote", out, [size_of(p) for p in pngs])
