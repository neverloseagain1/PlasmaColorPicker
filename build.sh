#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cargo build --manifest-path "$project_dir/rust-helper/Cargo.toml" --release
mkdir -p "$project_dir/contents/bin"
cp "$project_dir/rust-helper/target/release/plasmacolorpicker-helper" \
    "$project_dir/contents/bin/plasmacolorpicker-helper"
rm -f "$project_dir/contents/bin/plasma-wallpaper-palette"
printf 'Built helper at %s\n' "$project_dir/contents/bin/plasmacolorpicker-helper"
