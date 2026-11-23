#!/usr/bin/env bash
# Test the installer with a fake release server and an isolated HOME.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/pkg"
for cmd in awk dirname find grep install ln mkdir mktemp mv rm sort tail tar; do
  ln -s "$(command -v "$cmd")" "$work/bin/$cmd"
done
cat > "$work/bin/uname" <<'SH'
#!/bin/bash
case "$1" in
  -s) echo Darwin ;;
  -m) echo arm64 ;;
  *) exit 1 ;;
esac
SH
cat > "$work/bin/curl" <<'SH'
#!/bin/bash
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    https://*) url=$1; shift ;;
    -o) destination=$2; shift 2 ;;
    *) shift ;;
  esac
done
printf '%s\n' "$url" >> "$TEST_LOG"
case "$url" in
  https://api.github.com/repos/webkubor/kyvault/releases\?per_page=100\&page=1)
    if [ "$TEST_SCENARIO" = fallback ]; then
      printf '[]\n'
    else
      /bin/cat "$TEST_FIXTURES/page1.json"
    fi ;;
  https://api.github.com/repos/webkubor/kyvault/releases\?per_page=100\&page=2)
    /bin/cat "$TEST_FIXTURES/page2.json" ;;
  https://github.com/webkubor/kyvault/releases/download/v2.10.0/kyvault-aarch64-apple-darwin.tar.gz)
    /bin/cp "$TEST_FIXTURES/pkg.tar.gz" "$destination" ;;
  https://github.com/webkubor/kyvault/releases/download/v2.9.0/kyvault-aarch64-apple-darwin.tar.gz)
    /bin/cp "$TEST_FIXTURES/older.tar.gz" "$destination" ;;
  https://github.com/webkubor/kyvault/releases/download/v2.8.0/kyvault-aarch64-apple-darwin.tar.gz)
    printf 'invalid archive' > "$destination" ;;
  https://github.com/webkubor/kyvault/releases/download/v2.7.0/kyvault-aarch64-apple-darwin.tar.gz)
    /bin/cp "$TEST_FIXTURES/pkg.tar.gz" "$destination" ;;
  *) exit 22 ;;
esac
SH
cat > "$work/bin/pipx" <<'SH'
#!/bin/bash
echo "unexpected legacy cleanup" >> "$TEST_LOG"
exit 1
SH
# Block the legacy /usr/local/bin shim probe even when running on a real Mac.
cat > "$work/bin/head" <<'SH'
#!/bin/bash
case "${!#}" in
  /usr/local/bin/kyvault) exit 1 ;;
  *) /usr/bin/head "$@" ;;
esac
SH
cat > "$work/bin/cargo" <<'SH'
#!/bin/bash
set -eu
echo "source fallback" >> "$TEST_LOG"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --root) root=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ "$TEST_SCENARIO" = fallback ]
/bin/mkdir -p "$root/bin"
printf '#!/bin/bash\necho "kyvault from-source"\n' > "$root/bin/kyvault"
/bin/chmod +x "$root/bin/kyvault"
SH
cat > "$work/bin/git" <<'SH'
#!/bin/bash
set -eu
[ "$TEST_SCENARIO" = fallback ]
echo "source clone" >> "$TEST_LOG"
/bin/mkdir -p "${!#}/rust"
SH
chmod +x "$work/bin/"{curl,uname,pipx,head,cargo,git}

printf '#!/bin/bash\necho "kyvault 2.10.0"\n' > "$work/pkg/kyvault"
chmod +x "$work/pkg/kyvault"
tar -czf "$work/pkg.tar.gz" -C "$work/pkg" kyvault
printf '#!/bin/bash\necho "kyvault 2.9.0"\n' > "$work/pkg/kyvault"
tar -czf "$work/older.tar.gz" -C "$work/pkg" kyvault

# A full page including desktop, drafts, prereleases, non-CLI tags and a
# stable CLI; page 2 has a newer stable CLI even though it was published earlier.
{
  printf '[\n'
  printf '%s,\n' '{"tag_name":"desktop-v1.0.0","draft":false,"prerelease":false,"body":"{\"tag_name\":\"v90.0.0\"}","author":{"tag_name":"v99.0.0"}}'
  printf '%s,\n' '{"draft":true,"tag_name":"v10.0.0","prerelease":false}'
  printf '%s,\n' '{"prerelease":true,"tag_name":"v9.0.0","draft":false}'
  printf '%s,\n' '{"tag_name":"v8.0.0-rc1","draft":false,"prerelease":false}'
  printf '%s,\n' '{"tag_name":"v7.0.0","prerelease":false}'
  printf '%s,\n' '{"tag_name":"v06.0.0","draft":false,"prerelease":false}'
  for ((i=0; i<93; i++)); do
    printf '%s,\n' '{"tag_name":"not-a-cli","draft":false,"prerelease":false}'
  done
  printf '%s\n' '{"prerelease":false,"draft":false,"tag_name":"v2.9.0"}]'
} > "$work/page1.json"
printf '%s\n' '[{"name":"CLI","tag_name":"v2.10.0","draft":false,"prerelease":false}]' > "$work/page2.json"

new_home() {
  mkdir -p "$work/$1/.local/bin"
  : > "$work/$1/log"
}
run_install() {
  local test=$1
  shift
  env -i HOME="$work/$test" PATH="$work/bin:/usr/bin:/bin" \
    TEST_LOG="$work/$test/log" TEST_FIXTURES="$work" TEST_SCENARIO="$test" \
    /bin/bash -s -- "$@" < "$root/install.sh" > "$work/$test/out" 2>&1
}
fail() {
  printf 'Installer regression failed: %s\n' "$1" >&2
  cat "$work/$1/out" >&2
  exit 1
}

new_home fresh
run_install fresh || fail fresh
[ "$("$work/fresh/.local/bin/ky" --version)" = 'kyvault 2.10.0' ] || fail fresh
grep -Fq 'releases?per_page=100&page=2' "$work/fresh/log" || fail fresh

new_home pin
run_install pin v2.9.0 || fail pin
[ "$("$work/pin/.local/bin/ky" --version)" = 'kyvault 2.9.0' ] || fail pin
! grep -Fq 'api.github.com' "$work/pin/log" || fail pin

new_home fallback
run_install fallback || fail fallback
[ "$("$work/fallback/.local/bin/ky" --version)" = 'kyvault from-source' ] || fail fallback
grep -Fq 'source clone' "$work/fallback/log" || fail fallback
grep -Fq 'source fallback' "$work/fallback/log" || fail fallback

for scenario in missing malformed bad_archive wrong_version; do
  new_home "$scenario"
  printf '#!/bin/bash\necho kept\n' > "$work/$scenario/.local/bin/kyvault"
  chmod +x "$work/$scenario/.local/bin/kyvault"
  case "$scenario" in
    missing) tag=v2.8.1 ;;
    malformed) tag=desktop-v0.2.6 ;;
    bad_archive) tag=v2.8.0 ;;
    wrong_version) tag=v2.7.0 ;;
  esac
  if run_install "$scenario" "$tag"; then fail "$scenario"; fi
  [ "$("$work/$scenario/.local/bin/kyvault" --version)" = kept ] || fail "$scenario"
  ! grep -Eq 'api.github.com|source fallback|legacy cleanup' "$work/$scenario/log" || fail "$scenario"
done
[ ! -s "$work/malformed/log" ] || fail malformed
printf 'Installer release-channel regressions passed\n'
