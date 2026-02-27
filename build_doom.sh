#!/bin/bash
set -e

ESPDIR="/mnt/c/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/esp"

cd /tmp
rm -rf fbdoom fbdoom.zip

echo "[1/6] Downloading fbDOOM..."
wget -q --show-progress "https://github.com/wojciech-graj/fbdoom/archive/refs/heads/master.zip" -O fbdoom.zip
echo "  zip size: $(stat -c%s fbdoom.zip) bytes"

echo "[2/6] Extracting..."
unzip -q fbdoom.zip
mv fbdoom-master fbdoom
ls fbdoom/

echo "[3/6] Building fbDOOM with musl (static, no-pie)..."
cd /tmp/fbdoom

# fbDOOM uses a Makefile; build with musl-gcc for static ELF
# Target: Linux /dev/fb0 framebuffer backend
make CC=musl-gcc \
     CFLAGS="-O2 -static -no-pie -fno-pie -DNORMALUNIX -DLINUX -DFBDEV" \
     LDFLAGS="-static -no-pie" \
     2>&1

# Some fbDOOM versions produce 'fbdoom', others 'doom'
if [ -f fbdoom ]; then
    DOOM_BIN=fbdoom
elif [ -f doom ]; then
    DOOM_BIN=doom
else
    echo "Build failed: no output binary found"
    ls -la
    exit 1
fi

echo "[4/6] Stripping binary..."
musl-strip $DOOM_BIN || true
file $DOOM_BIN
ls -lh $DOOM_BIN

echo "[5/6] Downloading shareware doom1.wad..."
# Chocolate-Doom shareware WAD (public domain shareware)
wget -q --show-progress \
    "https://distro.ibiblio.org/slitaz/sources/packages/d/doom1.wad" \
    -O doom1.wad 2>&1 || \
wget -q --show-progress \
    "http://www.doomworld.com/idgames/doom1.wad" \
    -O doom1.wad 2>&1 || true

if [ ! -s doom1.wad ]; then
    echo "  Primary mirror failed, trying archive.org..."
    wget -q --show-progress \
        "https://archive.org/download/DoomsharewareEpisode/doom.WAD" \
        -O doom1.wad 2>&1 || true
fi

if [ -s doom1.wad ]; then
    echo "  doom1.wad: $(stat -c%s doom1.wad) bytes"
else
    echo "  WARNING: Could not download doom1.wad automatically."
    echo "  Place doom1.wad manually in esp/ folder."
fi

echo "[6/6] Copying to esp/..."
cp $DOOM_BIN "$ESPDIR/doom.elf"
[ -s doom1.wad ] && cp doom1.wad "$ESPDIR/doom1.wad"

echo ""
echo "=== Done ==="
echo "esp/doom.elf -> $(ls -lh $ESPDIR/doom.elf)"
[ -f "$ESPDIR/doom1.wad" ] && echo "esp/doom1.wad -> $(ls -lh $ESPDIR/doom1.wad)"
echo ""
echo "In echOS terminal: launch doom.elf /doom1.wad"
