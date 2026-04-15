#!/usr/bin/env bash
# Fetch the NES test ROMs referenced by tests/blargg_tests.rs and tests/nestest.rs.
#
# Source: christopherpow/nes-test-roms on GitHub — a curated mirror of the
# ROMs linked from https://www.nesdev.org/wiki/Emulator_tests . Individual
# files are downloaded via raw.githubusercontent.com so we don't clone the
# whole ~200 MB repo.
#
# Usage:
#   scripts/fetch_test_roms.sh            # populate tests/test_roms/
#   scripts/fetch_test_roms.sh -f         # re-download even if present
#   scripts/fetch_test_roms.sh <dir>      # populate a custom directory
#
# Requires: curl.

set -euo pipefail

FORCE=0
DEST=""
while (( $# > 0 )); do
    case "$1" in
        -f|--force) FORCE=1 ;;
        -h|--help)
            sed -n '2,14p' "$0"
            exit 0
            ;;
        *) DEST="$1" ;;
    esac
    shift
done

if [[ -z "$DEST" ]]; then
    # Anchor to the repo root so the script works from any CWD.
    DEST="$(cd "$(dirname "$0")/.." && pwd)/tests/test_roms"
fi
mkdir -p "$DEST"

BASE="https://raw.githubusercontent.com/christopherpow/nes-test-roms/master"

# Returns 0 if the file should be fetched, 1 if skipped.
need_fetch() {
    if (( FORCE == 0 )) && [[ -s "$DEST/$1" ]]; then
        return 1
    fi
    return 0
}

fetch() {
    local src="$1" dst="$2"
    if ! need_fetch "$dst"; then
        printf '  [have] %s\n' "$dst"
        return 0
    fi
    printf '  [get]  %s\n' "$dst"
    # --fail makes a 404 return non-zero; --location follows redirects.
    if ! curl -fsSL "$BASE/$src" -o "$DEST/$dst.part"; then
        rm -f "$DEST/$dst.part"
        printf '         ↳ FAILED (%s)\n' "$BASE/$src" >&2
        return 1
    fi
    mv "$DEST/$dst.part" "$DEST/$dst"
}

echo "Fetching test ROMs into $DEST"
echo "(source: https://github.com/christopherpow/nes-test-roms — mirrors nesdev wiki)"
echo

echo "· nestest (Kevin Horton)"
fetch "other/nestest.nes"                "nestest.nes"
fetch "other/nestest.log"                "nestest.log"
fetch "other/nestest.txt"                "nestest.txt"

echo
echo "· Blargg instr_test-v5"
fetch "instr_test-v5/official_only.nes"  "official_only.nes"
for n in 01-basics 02-implied 03-immediate 04-zero_page 05-zp_xy \
         06-absolute 07-abs_xy 08-ind_x 09-ind_y 10-branches \
         11-stack 12-jmp_jsr 13-rts 14-rti 15-brk 16-special; do
    fetch "instr_test-v5/rom_singles/${n}.nes" "${n}.nes"
done

echo
echo "· Blargg instr_timing  (supersedes cpu_timing_test6; uses \$6000 protocol)"
fetch "instr_timing/instr_timing.nes"                 "instr_timing.nes"
fetch "instr_timing/rom_singles/1-instr_timing.nes"   "1-instr_timing.nes"
fetch "instr_timing/rom_singles/2-branch_timing.nes"  "2-branch_timing.nes"

echo
echo "· Blargg ppu_vbl_nmi"
fetch "ppu_vbl_nmi/ppu_vbl_nmi.nes"      "ppu_vbl_nmi.nes"

echo
echo "· Blargg sprite_hit_tests_2005.10.05  (11 sub-tests — no combined ROM exists)"
for n in 01.basics 02.alignment 03.corners 04.flip 05.left_clip \
         06.right_edge 07.screen_bottom 08.double_height \
         09.timing_basics 10.timing_order 11.edge_timing; do
    fetch "sprite_hit_tests_2005.10.05/${n}.nes" "sprite_hit_${n}.nes"
done

echo
echo "· Blargg oam_stress"
fetch "oam_stress/oam_stress.nes"        "oam_stress.nes"

echo
echo "· Blargg apu_test"
fetch "apu_test/apu_test.nes"            "apu_test.nes"

echo
echo "Done. Run: cargo test --release"
