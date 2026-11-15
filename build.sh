#!/bin/sh
# Build the Rust helper and generate the non-README update-check baseline.
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cargo build --manifest-path "$project_dir/rust-helper/Cargo.toml" --release
mkdir -p "$project_dir/contents/bin"
cp "$project_dir/rust-helper/target/release/plasmacolorpicker-helper" \
    "$project_dir/contents/bin/plasmacolorpicker-helper"
rm -f "$project_dir/contents/bin/plasma-wallpaper-palette"

baseline="$project_dir/contents/update-baseline.txt"
temporary_baseline="$baseline.tmp.$$"
trap 'rm -f -- "$temporary_baseline"' EXIT HUP INT TERM
: >"$temporary_baseline"

if git -C "$project_dir" rev-parse --show-toplevel >/dev/null 2>&1; then
    source_files=$(git -C "$project_dir" ls-files --cached --others --exclude-standard)
else
    source_files=$(find "$project_dir" \
        -path "$project_dir/.git" -prune -o \
        -path "$project_dir/rust-helper/target" -prune -o \
        -path "$project_dir/contents/bin" -prune -o \
        -type f -print | sed "s|^$project_dir/||")
fi

while IFS= read -r relative_path; do
    [ -n "$relative_path" ] || continue
    case "$relative_path" in
        contents/bin/*|rust-helper/target/*|contents/update-baseline.txt*)
            continue
            ;;
    esac
    filename=${relative_path##*/}
    case "$filename" in
        [Rr][Ee][Aa][Dd][Mm][Ee]*)
            continue
            ;;
    esac
    source_path="$project_dir/$relative_path"
    [ -f "$source_path" ] || continue

    if command -v git >/dev/null 2>&1; then
        blob_sha=$(git -C "$project_dir" hash-object "$source_path")
    else
        command -v sha1sum >/dev/null 2>&1 || {
            printf 'Cannot create update baseline: git or sha1sum is required.\n' >&2
            exit 1
        }
        file_size=$(wc -c <"$source_path" | tr -d ' ')
        blob_sha=$(
            {
                printf 'blob %s\0' "$file_size"
                cat "$source_path"
            } | sha1sum | awk '{print $1}'
        )
    fi
    printf '%s\t%s\n' "$blob_sha" "$relative_path" >>"$temporary_baseline"
done <<EOF
$source_files
EOF

if [ ! -s "$temporary_baseline" ]; then
    printf 'Cannot create update baseline: no source files were found.\n' >&2
    exit 1
fi
mv -f -- "$temporary_baseline" "$baseline"
trap - EXIT HUP INT TERM
printf 'Built helper at %s\n' "$project_dir/contents/bin/plasmacolorpicker-helper"
printf 'Created source update baseline at %s\n' "$baseline"
