#!/usr/bin/env bash
# Horizon V2 with the Paper plugin layer, for Linux, and the plugins the VPS server runs, all on today's Paper SDK.
# First: python tools/v2/paper_build.py NAME --stage-only (the patched tree in D:/Horizon/v2-paper/stage).
# Runs inside rust:1.99.1-bookworm with /horizon = D:\Horizon (read-only) and /out = the package folder:
#   docker run --rm -v D:\Horizon:/horizon:ro -v D:\Horizon\perf\linux-v2p:/out -v horizon-linux-work:/work \
#     -v horizon-linux-target:/target -v horizon-linux-cargo:/usr/local/cargo/registry rust:1.99.1-bookworm \
#     bash /horizon/v2/tools/v2/build_linux_paper.sh
set -euo pipefail
export CARGO_TERM_COLOR=never
apt-get update -qq >/dev/null && apt-get install -y -qq libasound2-dev libudev-dev pkg-config rsync python3 >/dev/null
mkdir -p /out/engine /out/plugins

# ---- 1. The engine: V2's patched stage (rsync by checksum: an edited-then-restored file must not look older).
if [ -z "${SKIP_ENGINE:-}" ]; then
rsync -rlD --checksum --delete --exclude target /horizon/v2-paper/stage/ /work/v2p/
cd /work/v2p
# The join turn: a joining player's spawn ask is answered with nobody else online (plugin-sweep/linux).
python3 /horizon/plugin-sweep/linux/engine_login_turn.py /work/v2p/crates/horizon-client/src/game/lan.rs
grep -rl "D:/Horizon" --include=Cargo.toml . | xargs -r sed -i "s#D:/Horizon#/horizon#g" || true
CARGO_TARGET_DIR=/target/v2p RUSTC_BOOTSTRAP=1 RUSTFLAGS="-Zthreads=8" cargo build --locked --profile fast --no-default-features -p horizon-client --bin horizon-client 2>&1 | grep -E "^error|Finished" | tail -3
cp /target/v2p/fast/horizon-client /out/engine/
fi

# ---- 2. Plugins on the Paper SDK (paper-engine/sdk), release profile.
H=/work/h3
rm -rf $H && mkdir -p $H/plugin-tools/jport $H/paper-engine $H/p
rsync -a --exclude target /horizon/plugin-tools/jport/jrt/ $H/plugin-tools/jport/jrt/
rsync -a --exclude target /horizon/plugin-tools/jport/jbridge/ $H/plugin-tools/jport/jbridge/
rsync -a --exclude target /horizon/paper-engine/sdk/ $H/paper-engine/sdk/
for sibling in /horizon/paper-engine-test/essentials*/; do
  rsync -a --exclude target "$sibling" "$H/p/$(basename "$sibling")/"
done
for crate in luckperms_dep coreprotect_sw tebex_sw; do
  rsync -a --exclude target --exclude '*.jar' "/horizon/plugin-ports/$crate/" "$H/p/$crate/"
done
find $H -name Cargo.toml -exec sed -i "s#D:/Horizon/fabric-port/mod-sdk#D:/Horizon/paper-engine/sdk#g; s#D:/Horizon#$H#g" {} +

build_one() {
  crate=$1; lib=$2
  cd "$H/p/$crate"
  if CARGO_TARGET_DIR=/target/p_$crate RUSTC_BOOTSTRAP=1 RUST_MIN_STACK=268435456 cargo rustc --lib --release ${FEATS:-} -- -Zthreads=4 >"/out/plugins/$crate.log" 2>&1; then
    cp "/target/p_$crate/release/lib$lib.so" /out/plugins/ && echo "plugin $crate: ok"
  else
    echo "plugin $crate: FAILED"; tail -20 "/out/plugins/$crate.log"
  fi
}
FEATS="--no-default-features --features chat,spawn,antibuild,protect" build_one essentials-bundle essentials_bundle &
build_one luckperms_dep luckperms_dep &
build_one coreprotect_sw coreprotect_sw &
build_one tebex_sw tebex_sw &
wait
ls -la /out/engine /out/plugins/*.so
echo "linux v2 paper build done"
