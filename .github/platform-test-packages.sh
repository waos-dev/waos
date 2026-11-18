#!/bin/sh
# The packages whose tests CI runs on one system, as `-p` arguments: those that say they run and
# are tested there, and whose code depends on the system. macOS runs every package's tests, so a
# package whose answer cannot differ by system is not run again on another.
#   .github/platform-test-packages.sh linux|windows
#
# A package says where it runs in its own Cargo.toml, with a comment beside a system it lacks and
# why (the app's shell, for one, waits for docs/proposals/2026-11-13-windows-and-linux.md):
#   [package.metadata.waos]
#   systems = ["macos", "linux", "windows"]
#
# Of those, a package depends on the system when its sources, tests or manifest
# - switch on the target (`cfg(target_os`, `cfg(windows)`, `cfg(unix)`, `[target.'cfg(...)']`), or
# - call the system's files, processes or sockets (`std::fs`, `std::process`, `std::net`,
#   `std::os`, their tokio forms, `tempfile`), whose behaviour differs with no `cfg` to say so:
#   what a rename does over an open file, what a path is.
# Both are read from the code, so a package that gains either is tested without anyone adding it
# here. A match in a comment counts, which only tests more.
system=${1:?"usage: platform-test-packages.sh linux|windows"}

target_switch='cfg[a-z_]*\(.*\b(target_os|target_family|windows|unix)\b|target\.'"'"'cfg\('
system_calls='std::(fs|process|net|os)\b|tokio::(fs|process|net)\b|tempfile'

depends_on_system() {
  grep -rIE "$target_switch|$system_calls" "$1" --include='*.rs' --include=Cargo.toml |
    grep -qvE ':[[:space:]]*//'
}

packages=$(
  cargo metadata --no-deps --format-version 1 |
    jq -r --arg system "$system" '.packages[]
      | select(.metadata.waos.systems // [] | index($system))
      | .name + " " + (.manifest_path | gsub("\\\\"; "/") | sub("/Cargo\\.toml$"; ""))' |
    tr -d '\r' |  # jq on Windows ends its lines with CRLF
    while read -r name folder; do
      [ -d "$folder" ] || { echo "no folder $folder for $name" >&2; exit 1; }
      depends_on_system "$folder" && echo "-p $name"
    done
) || exit 1
# `cargo test` with no `-p` runs the workspace's default members, so an empty answer is a failure.
[ -n "$packages" ] || { echo "no package to test on $system: is jq installed?" >&2; exit 1; }
echo "$packages"
