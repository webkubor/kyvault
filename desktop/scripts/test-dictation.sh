#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export KYVAULT_STORE_DIR="$tmp/store" KY_DESKTOP_DATA_DIR="$tmp/desktop"
if [[ -n ${KY_DICTATION_TEST_REV:-} ]]; then
  rev=$(git rev-parse --verify --end-of-options "${KY_DICTATION_TEST_REV}^{commit}")
  mkdir -p "$tmp/source/desktop/scripts"
  (cd "$(git rev-parse --show-toplevel)" && git archive "$rev" \
    desktop/src-tauri/src/{ky_dictation.m,ky_dictation.h,flash.m}) | tar -x -C "$tmp/source"
  cp test-dictation.m probe-dictation.m "$tmp/source/desktop/scripts/"
  cd "$tmp/source/desktop/scripts"
fi
xcrun clang -fobjc-arc -fblocks -Werror -framework AppKit -framework AVFoundation \
  -framework Speech -framework UniformTypeIdentifiers test-dictation.m -o "$tmp/fixture"
(cd "$tmp" && ./fixture)
if [[ ${1:-} == --probe ]]; then
  /usr/bin/say -v Samantha -o "$tmp/fixture.aiff" 'The purple garden has seven flowers.'
  xcrun clang -fobjc-arc -fblocks -Werror -framework Foundation -framework Speech \
    probe-dictation.m -o "$tmp/probe"
  (cd "$tmp" && ./probe fixture.aiff)
fi
