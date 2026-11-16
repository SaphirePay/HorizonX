#!/usr/bin/env bash
# Usage (after python tools/v2/paper_build.py NAME --stage-only):
#   docker run --rm -v D:\Horizon:/horizon:ro -v D:\Horizon\perf\linux-v2p:/out -v horizon-linux-work:/work \n#     -v horizon-linux-target:/target -v horizon-linux-cargo:/usr/local/cargo/registry rust:1.99.1-bookworm \n#     bash /horizon/v2/tools/v2/build_linux_engine.sh
# The HorizonsX engine only (step 1 of tools/v2/build_linux_paper.sh): the patched stage -> /out/engine/horizon-client.
# A failed compile copies nothing (the target's old binary must never be taken for the new one).
set -euo pipefail
export CARGO_TERM_COLOR=never
apt-get update -qq >/dev/null && apt-get install -y -qq libasound2-dev libudev-dev pkg-config rsync python3 >/dev/null
mkdir -p /out/engine
rsync -rlD --checksum --delete --exclude target /horizon/v2-paper/stage/ /work/v2p/
cd /work/v2p
python3 /horizon/plugin-sweep/linux/engine_login_turn.py /work/v2p/crates/horizon-client/src/game/lan.rs
grep -rl "D:/Horizon" --include=Cargo.toml . | xargs -r sed -i "s#D:/Horizon#/horizon#g" || true
start=$(date +%s)
rm -f /out/engine/horizon-client
if CARGO_TARGET_DIR=/target/v2p RUSTC_BOOTSTRAP=1 RUSTFLAGS="-Zthreads=8" cargo build --locked --profile fast --no-default-features -p horizon-client --bin horizon-client > /out/engine/cargo.log 2>&1; then
  cp /target/v2p/fast/horizon-client /out/engine/
  echo "engine built in $(( $(date +%s) - start )) s"
  ls -la /out/engine
else
  echo "ENGINE BUILD FAILED"
  grep -E "^error" -A12 /out/engine/cargo.log | head -80
  exit 1
fi
