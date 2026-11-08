#!/bin/sh
# Writes a ~100 MB text file into the workspace target dir (never the repo).
set -eu
out="$(cd "$(dirname "$0")/../.." && pwd)/target/s2-ui.big.txt"
mkdir -p "$(dirname "$out")"
perl -e 'for my $i (1..1_250_000) { printf "%08d  let value_%d = compute(%d, \"lorem ipsum dolor sit amet, consectetur\");\n", $i, $i, $i * 7 }' > "$out"
ls -lh "$out"
