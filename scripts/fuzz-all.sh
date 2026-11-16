#!/usr/bin/env bash
# REQ-TST-040 and DEC-2384: every fuzz target for a fixed time on each change, after replaying
# every input that has ever found a fault, with debug assertions and overflow checks (-a), since
# REQ-GEN-008 forbids an overflow and a release build hides one.
#   scripts/fuzz-all.sh SECONDS NIGHTLY
# The targets run side by side, half as many at once as the host has processors: a fuzzer is
# one process, and run one after another the 44 targets took 18m54s on 16 November 2026 while
# the host's other processors waited. Each target still fuzzes for SECONDS on a processor of
# its own. A target's output is kept in fuzz/target/fuzz-logs and printed when it fails.
set -euo pipefail
seconds="$1"
nightly="$2"
cd "$(dirname "$0")/../fuzz"

cargo "+$nightly" clippy --quiet -- -D warnings
# Built once, before the runs, so that they do not wait on each other for the build.
cargo "+$nightly" fuzz build -a

logs="target/fuzz-logs"
rm -rf "$logs"
mkdir -p "$logs"
jobs=$(( $(nproc) / 2 ))
[ "$jobs" -ge 1 ] || jobs=1

fuzz_one() {
    local target="$1"
    mkdir -p "corpus/$target" "regressions/$target"
    if cargo "+$nightly" fuzz run -a "$target" "corpus/$target" "regressions/$target" -- \
        -max_total_time="$seconds" -timeout=10 -rss_limit_mb=2048 >"$logs/$target.log" 2>&1; then
        echo "$target: $(tail -1 "$logs/$target.log")"
    else
        echo "$target: FAILED"
        tail -40 "$logs/$target.log"
        return 1
    fi
}
export -f fuzz_one
export seconds nightly logs

# xargs fails when any target does, after letting the others finish.
cargo "+$nightly" fuzz list | xargs -P "$jobs" -I{} bash -c 'fuzz_one "$1"' _ {}
