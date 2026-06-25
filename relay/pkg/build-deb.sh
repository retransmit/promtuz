#!/bin/sh
# Build the pzrelay .deb the ONLY supported way.
#
# cargo-zigbuild cross-links against an old glibc (2.28) and static-links
# libstdc++/libgcc, so the binary runs on Debian 10+ / Ubuntu 18.04+ and its
# only runtime dep is libc6 — which the package's `depends` is pinned to.
#
# Do NOT run a plain `cargo deb`: it rebuilds natively (current glibc +
# dynamic libstdc++), producing a binary that the pinned `depends` mislabels
# and that fails on older distros. Always go through this script / its steps.
#
# Prereqs: cargo install cargo-zigbuild cargo-deb ; rustup target add
#          x86_64-unknown-linux-gnu ; zig on PATH.

set -e

TARGET=x86_64-unknown-linux-gnu
GLIBC=2.28

cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"

cargo zigbuild --release -p relay --target "${TARGET}.${GLIBC}"
cargo deb -p relay --no-build --target "${TARGET}"

echo
echo "deb: target/${TARGET}/debian/"
ls -1 "target/${TARGET}/debian/"*.deb
