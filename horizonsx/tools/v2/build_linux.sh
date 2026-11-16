#!/usr/bin/env bash
# HorizonsX for Linux (the VPS comparison): the engine, the supervisor and the bots from this worktree, the same
# profile as tools/v2/build.py. Runs inside rust:1.99.1-bookworm with /horizon = D:\Horizon (read-only), /out = the
# package folder, /work and /target = docker volumes (horizon-linux-work, horizon-linux-target).
#   docker run --rm -v D:\Horizon:/horizon:ro -v D:\Horizon\perf\linux-v2:/out -v horizon-linux-work:/work \
#     -v horizon-linux-target:/target -v horizon-linux-cargo:/usr/local/cargo/registry rust:1.99.1-bookworm \
#     bash /horizon/v2/tools/v2/build_linux.sh
set -euo pipefail
export CARGO_TERM_COLOR=never
apt-get update -qq >/dev/null && apt-get install -y -qq libasound2-dev libudev-dev pkg-config rsync >/dev/null
# rsync by checksum: a file put back after an edit must not look older than its build.
rsync -rlD --checksum --delete --exclude target --exclude .git /horizon/v2/ /work/v2/
cd /work/v2
export CARGO_TARGET_DIR=/target/v2 RUSTC_BOOTSTRAP=1 RUSTFLAGS="-Zthreads=8"
cargo build --locked --profile fast --no-default-features -p horizon-client --bin horizon-client 2>&1 | grep -E "^error|Finished" | tail -3
cargo build --locked --profile fast -p horizon-server -p horizon-bots 2>&1 | grep -E "^error|Finished" | tail -3
mkdir -p /out
cp /target/v2/fast/horizon-client /target/v2/fast/horizon-server /target/v2/fast/horizon-bots /out/
ls -la /out
echo "linux v2 build done"
