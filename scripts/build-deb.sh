#!/bin/sh
# Build a Promtuz daemon .deb the ONLY supported way: cargo-zigbuild (links an
# old glibc + static libstdc++/libgcc) → cargo-deb. The result runs on
# Debian 10+ / Ubuntu 18.04+ with libc6 as its only dependency.
#
#   ./scripts/build-deb.sh relay
#   ./scripts/build-deb.sh resolver
#
# Do NOT run a plain `cargo deb`: it rebuilds against the host glibc (+ dynamic
# libstdc++), producing a binary the pinned `depends = libc6` mislabels and that
# fails on older distros.
#
# Prereqs: cargo install cargo-zigbuild cargo-deb ; rustup target add
#          x86_64-unknown-linux-gnu ; zig on PATH.

set -e

CRATE="${1:?usage: build-deb.sh <relay|resolver>}"
TARGET=x86_64-unknown-linux-gnu
GLIBC=2.28

cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"

cargo zigbuild --release -p "$CRATE" --target "${TARGET}.${GLIBC}"
cargo deb -p "$CRATE" --no-build --target "${TARGET}"

echo
echo "deb: target/${TARGET}/debian/"
ls -1 "target/${TARGET}/debian/"*.deb
