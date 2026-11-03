#!/usr/bin/env bash
# Builds the crates for every target of REQ-GEN-015 and prints a result
# table; exit status 1 if any target fails. Runs on the Linux build host
# (scripts/check.sh full runs it there through NEOTELETEXT_REMOTE).
#
# Mode "build" compiles and links; "test" also runs the tests under qemu
# user-mode emulation; "check" compiles without linking or code generation,
# for targets whose SDK cannot exist on this host (Apple, Windows MSVC and
# ESP-IDF). The Apple targets' neottx program is built and tested on the Mac by
# scripts/check.sh full. neottx for Windows is built with the MinGW toolchain,
# because its TLS library's C code needs the Windows SDK headers under MSVC
# (IMPLEMENTATION-LOG.md decision 39).
set -uo pipefail
cd "$(dirname "$0")/.."
. "$HOME/.cargo/env"

# REQ-GEN-001: the pinned toolchain (rust-toolchain.toml).
readonly RUST="1.99.0"
# Espressif's fork for Xtensa, installed with espup: 1.94 and 1.95 fail
# release builds (esp-rs/rust#277), so 1.93 is used, below the workspace's
# rust-version (IMPLEMENTATION-LOG.md decision 38).
readonly ESP="esp"
# The fork espup installed, pinned (REQ-SEC-014, REQ-GEN-001): its version
# names the source commit, and its compiler driver's SHA-256 the build. A
# different one fails every Xtensa row, before anything is built with it.
readonly ESP_VERSION="rustc 1.93.0-nightly (2b43689c5 2026-01-27) (1.93.0.0)"
readonly ESP_DRIVER_SHA256="e3445e15d6ab391bb6e257fa7fc4523c423e17ef72190f323779601bfffb15d0"
# REQ-GEN-001: the stated minimum version, which scripts/tools.sh pins and
# installs. Read, not sourced: that file stops at the first failure, and this
# script counts failures and goes on.
MSRV_TOOLCHAIN="$(sed -n 's/^readonly MSRV_TOOLCHAIN="\(.*\)"$/\1/p' scripts/tools.sh)"
readonly MSRV_TOOLCHAIN

# The crates that run on bare metal (REQ-GEN-017), the pair built for
# WebAssembly, and the std crates for operating systems without a terminal.
# The audio profile's PCM16 and IMA ADPCM are named, though in its default,
# because microcontroller ports keep them (decision E.131).
readonly BARE="-p neoteletext-core -p neoteletext-formats -p neoteletext-slicer -p neoteletext-web -p neoteletext-image -p neoteletext-audio --features neoteletext-audio/pcm16,neoteletext-audio/ima"
# The audio profile as the desktops build it: Opus alone, PCM16 and IMA
# ADPCM compiled out (decision E.131).
readonly AUDIO_DESKTOP="-p neoteletext-audio --no-default-features --features idl,opus"
readonly WASM="-p neoteletext-core -p neoteletext-formats"
readonly LIBRARIES_STD="$BARE --features neoteletext-formats/std,neoteletext-image/std"
# Everything except the host-only xtask: the libraries and the neottx program.
readonly ALL="--workspace --exclude xtask"

# target | crates | mode | toolchain | extra flags
readonly MATRIX="
thumbv6m-none-eabi|$BARE|build|$RUST|
thumbv7em-none-eabihf|$BARE|build|$RUST|
thumbv8m.main-none-eabihf|$BARE|build|$RUST|
riscv32imc-unknown-none-elf|$BARE|build|$RUST|
riscv32imac-unknown-none-elf|$BARE|build|$RUST|
riscv32imafc-unknown-none-elf|$BARE|build|$RUST|
xtensa-esp32-none-elf|$BARE|build|$ESP|-Zbuild-std=core --ignore-rust-version
xtensa-esp32s3-none-elf|$BARE|build|$ESP|-Zbuild-std=core --ignore-rust-version
xtensa-esp32-espidf|$LIBRARIES_STD|check|$ESP|-Zbuild-std=std,panic_abort --ignore-rust-version
xtensa-esp32s3-espidf|$LIBRARIES_STD|check|$ESP|-Zbuild-std=std,panic_abort --ignore-rust-version
x86_64-unknown-none|$BARE|build|$RUST|
x86_64-unknown-linux-gnu|$ALL|build|$RUST|
aarch64-unknown-linux-gnu|$ALL|build|$RUST|
armv7-unknown-linux-gnueabihf|$ALL|build|$RUST|
riscv64gc-unknown-linux-gnu|$ALL|build|$RUST|
aarch64-unknown-linux-gnu|$BARE --all-features|test|$RUST|
armv7-unknown-linux-gnueabihf|$BARE --all-features|test|$RUST|
riscv64gc-unknown-linux-gnu|$BARE --all-features|test|$RUST|
x86_64-unknown-linux-gnu|$AUDIO_DESKTOP|test|$RUST|
x86_64-pc-windows-msvc|$AUDIO_DESKTOP|check|$RUST|
aarch64-apple-darwin|$AUDIO_DESKTOP|check|$RUST|
x86_64-pc-windows-msvc|$LIBRARIES_STD|check|$RUST|
x86_64-pc-windows-gnu|$ALL|build|$RUST|
aarch64-apple-darwin|$LIBRARIES_STD|check|$RUST|
x86_64-apple-darwin|$LIBRARIES_STD|check|$RUST|
wasm32-unknown-unknown|$WASM|build|$RUST|
wasm32-wasip1|$WASM|build|$RUST|
x86_64-unknown-linux-gnu|$ALL --all-features|build|$MSRV_TOOLCHAIN|
"

# Two places name the minimum version; they must agree.
stated="$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)"
if [[ "$MSRV_TOOLCHAIN" != "$stated" && "$MSRV_TOOLCHAIN" != "$stated.0" ]]; then
    echo "scripts/tools.sh's MSRV_TOOLCHAIN $MSRV_TOOLCHAIN is not Cargo.toml's rust-version $stated" >&2
    exit 1
fi

# The Xtensa fork, checked before it builds anything.
esp_problem=""
if rustup toolchain list | grep -q "^$ESP"; then
    esp_found="$(rustc "+$ESP" --version 2>/dev/null)"
    esp_driver="$(ls "$(rustup run "$ESP" rustc --print sysroot)"/lib/librustc_driver-*.so 2>/dev/null | head -n1)"
    esp_sha256="$( [ -n "$esp_driver" ] && sha256sum "$esp_driver" | cut -d' ' -f1)"
    if [ "$esp_found" != "$ESP_VERSION" ]; then
        esp_problem="toolchain $ESP is \"$esp_found\", not the pinned \"$ESP_VERSION\""
    elif [ "$esp_sha256" != "$ESP_DRIVER_SHA256" ]; then
        esp_problem="toolchain $ESP's compiler driver has SHA-256 $esp_sha256, not the pinned $ESP_DRIVER_SHA256"
    fi
fi

# qemu runs the cross-compiled tests, with each target's C library.
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu"
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_RUNNER="qemu-arm -L /usr/arm-linux-gnueabihf"
export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_RUNNER="qemu-riscv64 -L /usr/riscv64-linux-gnu"

# Every standard library the matrix needs, for the pinned toolchain.
rustup -q target add --toolchain "$RUST" $(sed -n 's/^\([^|]*\)|.*|'"$RUST"'|.*$/\1/p' <<<"$MATRIX" | sort -u) >/dev/null

mkdir -p target/build-targets
failures=0
# Rows are numbered in their logs' names: a target may have several rows.
row=0
printf '%-32s %-6s %s\n' TARGET MODE RESULT
while IFS='|' read -r target crates mode toolchain extra; do
    [ -z "$target" ] && continue
    row=$((row + 1))
    log="target/build-targets/$row.$target.$mode.log"
    # A row that cannot run fails, as DEVELOPMENT.md section 9.2 says of any step.
    if ! rustup toolchain list | grep -q "^$toolchain"; then
        printf '%-32s %-6s %s\n' "$target" "$mode" "FAILED: toolchain $toolchain is not installed; rustup or espup installs it"
        failures=$((failures + 1))
        continue
    fi
    if [ "$toolchain" = "$ESP" ] && [ -n "$esp_problem" ]; then
        printf '%-32s %-6s %s\n' "$target" "$mode" "FAILED: $esp_problem"
        failures=$((failures + 1))
        continue
    fi
    # This host's own target has no runner.
    runner_variable="CARGO_TARGET_$(tr 'a-z.-' 'A-Z__' <<<"$target")_RUNNER"
    runner="${!runner_variable:-}"
    if [ "$mode" = test ] && [ -n "$runner" ] && ! command -v "${runner%% *}" >/dev/null; then
        printf '%-32s %-6s %s\n' "$target" "$mode" "FAILED: ${runner%% *} is not installed; the host's package manager installs qemu-user"
        failures=$((failures + 1))
        continue
    fi
    # shellcheck disable=SC2086 # the crate list and flags are split on purpose
    if cargo "+$toolchain" "$mode" --release --target "$target" $crates $extra >"$log" 2>&1; then
        result=OK
    else
        result="FAILED: $(grep -m1 -E '^error' "$log" || tail -n1 "$log")"
        failures=$((failures + 1))
    fi
    printf '%-32s %-6s %s\n' "$target" "$mode" "$result"
done <<<"$MATRIX"
exit $((failures > 0))
