#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
shell_dir="$repo_root/userspace/zig/shell"
tmpfs_foo="$repo_root/tmpfs/foo"

(
    cd "$shell_dir"
    zig build --release=small
)

mkdir -p "$tmpfs_foo"
install -m 0755 "$shell_dir/zig-out/bin/shell" "$tmpfs_foo/shell"
install -m 0755 "$shell_dir/zig-out/bin/spawned_prog" "$tmpfs_foo/spawned_prog"

cd "$repo_root"
cargo run \
    -p vsfs \
    --features host-tool \
    --bin tool \
    --target x86_64-unknown-linux-gnu \
    -- \
    --root tmpfs \
    --output disk.img
