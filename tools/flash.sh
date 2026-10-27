#!/bin/sh
# Build, pack and flash the firmware. If it is running, it is asked over USB to
# reboot into the bootloader; otherwise hold Up + Down for two seconds (or
# HOME/BOOT + RESET) and this waits for the RP2350 drive.
#
# Environment: PYTHON, a Python with pyserial (default python3); MOUNT, where the
# RP2350 drive mounts (default /run/media/$USER/RP2350).
set -eu
cd "$(dirname "$0")/.."
: "${TMPDIR:=$HOME/.cache/rig-tmp}"
: "${CARGO_TARGET_DIR:=$TMPDIR/badger-target}"
: "${PYTHON:=python3}"
: "${MOUNT:=/run/media/$USER/RP2350}"
export TMPDIR CARGO_TARGET_DIR
mkdir -p "$TMPDIR"

cargo build --release -j 2
llvm-objcopy -O binary "$CARGO_TARGET_DIR/thumbv8m.main-none-eabihf/release/badger-ratatui" "$TMPDIR/badger.bin"
python3 tools/uf2.py "$TMPDIR/badger.bin" badger-ratatui.uf2

if [ ! -f "$MOUNT/INFO_UF2.TXT" ] && ls /dev/serial/by-id/*badger-ratatui* >/dev/null 2>&1; then
    "$PYTHON" tools/badge.py send bootsel || true
fi
echo "waiting for $MOUNT"
i=0
while [ ! -f "$MOUNT/INFO_UF2.TXT" ]; do
    i=$((i + 1))
    [ "$i" -le 120 ] || { echo "no RP2350 drive after 2 minutes" >&2; exit 1; }
    sleep 1
done
cp badger-ratatui.uf2 "$MOUNT/"
sync
echo flashed
