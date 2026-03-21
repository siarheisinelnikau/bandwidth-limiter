"""Render embedded EXE icons with Win32 DrawIconEx; never launch or capture an app.
Usage: python scripts/verify-icons.py ABSOLUTE_EXE ABSOLUTE_OUTPUT_DIRECTORY
Requires Pillow only for PNG encoding and the contact sheet.
"""
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
from pathlib import Path
import struct
import sys
from PIL import Image, ImageDraw

exe, output = map(lambda value: Path(value).resolve(), sys.argv[1:])
output.mkdir(parents=True, exist_ok=True)
binary = exe.read_bytes()
pe = struct.unpack_from("<I", binary, 0x3c)[0]
assert binary[pe:pe+4] == b"PE\0\0"
machine = struct.unpack_from("<H", binary, pe+4)[0]
optional = pe+24
subsystem = struct.unpack_from("<H", binary, optional+68)[0]
assert machine == 0x8664 and subsystem in (2, 3), "Expected an x64 Windows executable"
sections_at = optional + struct.unpack_from("<H", binary, pe+20)[0]
sections = [struct.unpack_from("<IIII", binary, sections_at+40*i+8) for i in range(struct.unpack_from("<H", binary, pe+6)[0])]
def raw(rva):
    for virtual_size, address, raw_size, offset in sections:
        if address <= rva < address+max(virtual_size,raw_size):
            return offset+rva-address
    raise ValueError("Invalid PE resource/import address")
imports = []
imports_rva = struct.unpack_from("<I", binary, optional+120)[0]
if imports_rva:
    descriptor = raw(imports_rva)
    while any(binary[descriptor:descriptor+20]):
        name = raw(struct.unpack_from("<I", binary, descriptor+12)[0])
        imports.append(binary[name:binary.index(b"\0",name)].decode("ascii"))
        descriptor += 20
assert not {name.lower() for name in imports} & {"opengl32.dll", "d3d11.dll", "d3d12.dll", "dxgi.dll"}, "Unexpected GPU renderer import"
kernel = c.WinDLL("kernel32", use_last_error=True)
user = c.WinDLL("user32", use_last_error=True)
gdi = c.WinDLL("gdi32", use_last_error=True)
def bind(dll, name, result, arguments):
    fn = getattr(dll, name)
    fn.restype, fn.argtypes = result, arguments
    return fn
load = bind(kernel, "LoadLibraryExW", w.HMODULE, [w.LPCWSTR, w.HANDLE, w.DWORD])
free = bind(kernel, "FreeLibrary", w.BOOL, [w.HMODULE])
find = bind(kernel, "FindResourceW", w.HRSRC, [w.HMODULE, c.c_void_p, c.c_void_p])
load_resource = bind(kernel, "LoadResource", w.HGLOBAL, [w.HMODULE, w.HRSRC])
lock = bind(kernel, "LockResource", c.c_void_p, [w.HGLOBAL])
size_resource = bind(kernel, "SizeofResource", w.DWORD, [w.HMODULE, w.HRSRC])
load_icon = bind(user, "LoadImageW", w.HANDLE, [w.HINSTANCE, c.c_void_p, w.UINT, c.c_int, c.c_int, w.UINT])
destroy = bind(user, "DestroyIcon", w.BOOL, [w.HICON])
create_dc = bind(gdi, "CreateCompatibleDC", w.HDC, [w.HDC])
delete_dc = bind(gdi, "DeleteDC", w.BOOL, [w.HDC])
select = bind(gdi, "SelectObject", w.HGDIOBJ, [w.HDC, w.HGDIOBJ])
delete = bind(gdi, "DeleteObject", w.BOOL, [w.HGDIOBJ])
dib = bind(gdi, "CreateDIBSection", w.HBITMAP, [w.HDC, c.c_void_p, w.UINT, c.POINTER(c.c_void_p), w.HANDLE, w.DWORD])
draw = bind(user, "DrawIconEx", w.BOOL, [w.HDC, c.c_int, c.c_int, w.HICON, c.c_int, c.c_int, w.UINT, w.HBRUSH, w.UINT])
flush = bind(gdi, "GdiFlush", w.BOOL, [])
module = load(str(exe), None, 0x22)  # data/image resources only, no DllMain/code execution
if not module:
    raise c.WinError(c.get_last_error())
try:
    resource = find(module, 1, 14)
    if not resource:
        raise RuntimeError("The executable has no application icon group")
    data = c.string_at(lock(load_resource(module, resource)), size_resource(module, resource))
    count = struct.unpack_from("<H", data, 4)[0]
    sizes = [struct.unpack_from("<BB", data, 6 + i * 14) for i in range(count)]
    sizes = [(a or 256, b or 256) for a, b in sizes]
    assert all((n, n) in sizes for n in [16, 20, 24, 32, 48, 64, 128, 256]), sizes
    sheet = Image.new("RGB", (720, 900), "#e4e4e4")
    labels = ImageDraw.Draw(sheet)
    reports = []
    next_y = 0
    for row, n in enumerate([16, 20, 24, 32, 48, 64]):
        icon = load_icon(module, 1, 1, n, n, 0)
        if not icon:
            raise c.WinError(c.get_last_error())
        try:
            for column, background in enumerate([(250, 250, 250), (28, 31, 38)]):
                dc = create_dc(None)
                bits = c.c_void_p()
                header = c.create_string_buffer(struct.pack("<IiiHHIIiiII", 40, n, -n, 1, 32, 0, n*n*4, 0, 0, 0, 0))
                bitmap = dib(dc, header, 0, c.byref(bits), None, 0)
                if not bitmap:
                    raise c.WinError(c.get_last_error())
                previous = select(dc, bitmap)
                try:
                    bg = bytes([background[2], background[1], background[0], 255]) * (n*n)
                    c.memmove(bits, bg, len(bg))
                    if not draw(dc, 0, 0, icon, n, n, 0, None, 3):
                        raise c.WinError(c.get_last_error())
                    flush()
                    rgba = c.string_at(bits, n*n*4)
                    pixels = Image.frombytes("RGBA", (n,n), rgba, "raw", "BGRA").convert("RGB")
                    changed = [(x,y) for y in range(n) for x in range(n) if pixels.getpixel((x,y)) != background]
                    assert changed, f"Blank packaged icon at {n}px"
                    bounds = [min(x for x,y in changed), min(y for x,y in changed), max(x for x,y in changed)+1, max(y for x,y in changed)+1]
                    pixels.save(output / f"embedded-{n}-{'light' if column == 0 else 'dark'}.png")
                    x,y = column*350+12, next_y+22
                    labels.text((x,y-17), f"{n}px native and 3x / bounds {bounds}", fill="black")
                    sheet.paste(pixels, (x,y))
                    sheet.paste(pixels.resize((n*3,n*3), Image.Resampling.NEAREST), (x+80,y))
                    reports.append({"size":n,"background":background,"visible_bounds":bounds,"changed_pixels":len(changed)})
                finally:
                    select(dc, previous)
                    delete(bitmap)
                    delete_dc(dc)
        finally:
            destroy(icon)
        next_y += max(85, n*3+35)
    sheet.save(output / "embedded-icons.png")
    report = {"executable":str(exe),"machine":"x64","subsystem":subsystem,"imports":imports,"sha256":hashlib.sha256(exe.read_bytes()).hexdigest(),"resource_sizes":sizes,"renders":reports,"scope":"LoadLibraryEx resource-only + LoadImageW + DrawIconEx, no process launch or OS tray screenshot"}
    (output / "icons.json").write_text(json.dumps(report, indent=2))
    print(json.dumps({"sizes":sizes,"proof":str(output / "embedded-icons.png")}))
finally:
    free(module)
