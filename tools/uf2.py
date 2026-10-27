#!/usr/bin/env python3
"""Pack a flat binary into a UF2 for the RP2350 boot ROM.

    uf2.py app.bin app.uf2                 # an RP2350 Arm image at the start of flash
    uf2.py --absolute flash.bin flash.uf2  # a whole-flash image, written where it says

In a whole-flash image, pages that are all 0xFF past the first 2 MiB are skipped,
so it stays small; the first 2 MiB, where an app image lives, are always written
so nothing of one survives a restore.
"""
import argparse
import struct

FLASH_BASE = 0x10000000
FAMILY_RP2350_ARM_S = 0xE48BFF59
FAMILY_ABSOLUTE = 0xE48BFF57
PAGE = 256
ALWAYS_WRITE = 2 * 1024 * 1024


def pack(data: bytes, family: int, skip_blank: bool) -> bytes:
    data += b"\xff" * (-len(data) % PAGE)
    pages = [
        (off, data[off : off + PAGE])
        for off in range(0, len(data), PAGE)
        if not (skip_blank and off >= ALWAYS_WRITE and data[off : off + PAGE] == b"\xff" * PAGE)
    ]
    out = bytearray()
    for n, (off, page) in enumerate(pages):
        header = struct.pack(
            "<IIIIIIII",
            0x0A324655,  # magic 0
            0x9E5D5157,  # magic 1
            0x00002000,  # flags: family ID present
            FLASH_BASE + off,
            PAGE,
            n,
            len(pages),
            family,
        )
        out += header + page + b"\x00" * (476 - PAGE) + struct.pack("<I", 0x0AB16F30)
    return bytes(out)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--absolute", action="store_true", help="whole-flash image (absolute family)")
    ap.add_argument("input")
    ap.add_argument("output")
    a = ap.parse_args()
    data = open(a.input, "rb").read()
    uf2 = pack(data, FAMILY_ABSOLUTE if a.absolute else FAMILY_RP2350_ARM_S, skip_blank=a.absolute)
    open(a.output, "wb").write(uf2)
    print(f"{a.output}: {len(uf2) // 512} blocks, {len(uf2)} bytes")


if __name__ == "__main__":
    main()
