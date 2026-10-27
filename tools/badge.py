#!/usr/bin/env python3
"""Talk to the firmware's USB serial port.

    badge.py stats [SECONDS]   print the once-a-second frame timing reports
    badge.py send COMMAND      send `bootsel` or `screen`
"""
import glob
import sys
import time

import serial


def port() -> str:
    found = glob.glob("/dev/serial/by-id/*badger-ratatui*")
    if not found:
        sys.exit("no badger-ratatui serial port")
    return found[0]


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    s = serial.Serial(port(), timeout=1)
    if sys.argv[1] == "send" and len(sys.argv) == 3:
        s.write(sys.argv[2].encode() + b"\n")
        s.flush()
        return
    if sys.argv[1] == "stats":
        end = time.time() + float(sys.argv[2] if len(sys.argv) > 2 else 5)
        s.reset_input_buffer()
        while time.time() < end:
            line = s.readline().decode(errors="replace").strip()
            if line:
                print(line, flush=True)
        return
    sys.exit(__doc__)


if __name__ == "__main__":
    main()
