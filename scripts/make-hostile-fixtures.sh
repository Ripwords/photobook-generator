#!/usr/bin/env bash
# Generates the hostile fixtures that are pure byte manipulation and need no
# image library (ImageMagick/exiftool are not installed and must not be
# brew-installed). The three fixtures that need real image data
# (one-pixel.jpg, cmyk.jpg, no-extension) are generated instead by
# `swift scripts/make-fixtures.swift` -- run that too before running the
# hostile-input test suite.
#
# NOTE: the fixtures this script writes are committed to git for a
# reproducible test corpus, but nothing checks automatically that the
# committed bytes still match what this script would produce. If you edit
# this script, rerun it and re-commit the regenerated files in
# sidecar/Fixtures/hostile/.
set -euo pipefail
DIR="sidecar/Fixtures/hostile"
mkdir -p "$DIR"

: > "$DIR/empty.jpg"                                    # zero bytes
head -c 400 sidecar/Fixtures/landscape.jpg > "$DIR/truncated.jpg"
printf 'not an image at all, just text' > "$DIR/text.jpg"
head -c 2000 /dev/urandom > "$DIR/random.jpg"

# corrupt-scan-data.jpg: a copy of landscape.jpg with its JPEG header, SOF,
# and DHT segments left intact (landscape.jpg's SOS marker is at byte 751)
# but the Huffman-coded scan data bit-flipped. This is a different failure
# class from truncated.jpg (a clean cut -- ImageIO sees a short read) and
# random.jpg (no valid header at all -- ImageIO rejects it immediately): it
# is the classic "decoder walks off the end of a corrupt Huffman table"
# crash surface that malformed real-world photos actually exercise.
cp sidecar/Fixtures/landscape.jpg "$DIR/corrupt-scan-data.jpg"
dd if=/dev/urandom of="$DIR/corrupt-scan-data.jpg" bs=1 seek=900 count=4000 conv=notrunc status=none

echo "created byte-level hostile fixtures in $DIR"
echo "run 'swift scripts/make-fixtures.swift' for the image-based ones"
