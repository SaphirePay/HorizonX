#!/usr/bin/env bash
# The Java layer's server plugin for Linux: libhorizon_java.so, built in Docker from this checkout (read-only).
#
#   docker run --rm -v D:\Horizon:/horizon:ro -v D:\Horizon\perf\linux-javalayer:/out \
#     -v horizon-linux-work:/work -v horizon-linux-target:/target -v horizon-linux-cargo:/usr/local/cargo/registry \
#     rust:1.99.1-bookworm bash /horizon/paper-engine/java-layer/linux/build_layer.sh
#
# Out: /out/plugins/libhorizon_java.so and /out/java-layer/lib (java/out/lib: the layer's jar and its libraries, the
# same on every system), so /out is laid out as a server folder takes it (plugins/, java-layer/lib/).
set -euo pipefail
export CARGO_TERM_COLOR=never
apt-get update -qq >/dev/null && apt-get install -y -qq rsync pkg-config >/dev/null
SRC=/work/jl/Horizon
mkdir -p "$SRC/paper-engine/java-layer" "$SRC/plugin-tools/jport" /out/plugins /out/java-layer
rsync -rlD --checksum --delete --exclude target /horizon/paper-engine/java-layer/rust/ "$SRC/paper-engine/java-layer/rust/"
rsync -rlD --checksum --delete --exclude target /horizon/paper-engine/sdk/ "$SRC/paper-engine/sdk/"
for crate in jrt jbridge; do
  rsync -rlD --checksum --delete --exclude target "/horizon/plugin-tools/jport/$crate/" "$SRC/plugin-tools/jport/$crate/"
done
# The Windows checkout's absolute paths, made this tree's.
grep -rl "D:/Horizon" --include=Cargo.toml "$SRC" | xargs -r sed -i "s#D:/Horizon#$SRC#g"
cd "$SRC/paper-engine/java-layer/rust"
# The Windows lock file pins the same crates; a path dependency's new home changes nothing in it.
CARGO_TARGET_DIR=/target/javalayer cargo build --release 2>&1 | grep -E "^(error|warning: unused)|Finished" | tail -5
cp /target/javalayer/release/libhorizon_java.so /out/plugins/
rsync -rlD --checksum --delete /horizon/paper-engine/java-layer/java/out/lib/ /out/java-layer/lib/
ls -la /out/plugins
du -sh /out/java-layer/lib
echo "java layer build done"
