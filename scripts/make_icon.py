import struct, zlib

w = h = 128
rows = b''
for y in range(h):
    row = b'\x00'
    for x in range(w):
        cx, cy = x - 64, y - 60
        shaft = abs(cx) <= 14 and -50 <= cy <= 10
        head = abs(cx) <= 40 - (cy - 10) and 10 <= cy <= 50
        arrow = shaft or head
        circle = cx * cx + (y - 64) ** 2 <= 62 * 62
        if arrow and circle:
            row += b'\x60\xcd\xff\xff'
        elif circle:
            row += b'\x2b\x2b\x2b\xff'
        else:
            row += b'\x00\x00\x00\x00'
    rows += row

def chunk(t, d):
    c = t + d
    return len(d).to_bytes(4, 'big') + c + zlib.crc32(c).to_bytes(4, 'big')

png = (b'\x89PNG\r\n\x1a\n'
       + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
       + chunk(b'IDAT', zlib.compress(rows))
       + chunk(b'IEND', b''))
open('extension/icon128.png', 'wb').write(png)
print('icon written', len(png))
