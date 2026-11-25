#!/usr/bin/env bash
# Isolated release fixtures: no network, compilation, or real vault access.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT
export HOME="$TEST_DIR/home" KYVAULT_INSTALL_DIR="$TEST_DIR/bin"
export FIXTURE_DIR="$TEST_DIR" REAL_HEAD
REAL_HEAD=$(command -v head)
mkdir -p "$HOME/.keyring/module/kyvault" "$KYVAULT_INSTALL_DIR" "$TEST_DIR/tools" "$TEST_DIR/pkg"
printf 'keep vault\n' > "$HOME/.keyring/master.key"
printf 'keep module\n' > "$HOME/.keyring/module/kyvault/marker"
cat > "$TEST_DIR/tools/curl" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FIXTURE_DIR/requests"
output=''
for ((i=1; i<=$#; i++)); do
  if [ "${!i}" = -o ]; then j=$((i+1)); output=${!j}; fi
done
if [ -z "$output" ]; then
  printf '[{"tag_name":"desktop-v9.9.9"},{"tag_name":"v2.3.4"},{"tag_name":"v2.3.3"}]\n'
else
  [ "${FAIL_DOWNLOAD:-0}" != 1 ] || exit 22
  cp "$FIXTURE_DIR/package.tar.gz" "$output"
fi
SH
cat > "$TEST_DIR/tools/uname" <<'SH'
#!/usr/bin/env bash
case "$1" in -s) echo Linux;; -m) echo x86_64;; esac
SH
cat > "$TEST_DIR/tools/head" <<'SH'
#!/usr/bin/env bash
# Never inspect or remove the machine's legacy Python entry point.
case "$*" in *'/usr/local/bin/kyvault'*) exit 0;; esac
exec "$REAL_HEAD" "$@"
SH
for tool in pipx cargo git sudo; do
  cat > "$TEST_DIR/tools/$tool" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$0 $*" >> "$FIXTURE_DIR/side-effects"
exit 1
SH
done
chmod +x "$TEST_DIR/tools/"*
export PATH="$TEST_DIR/tools:$PATH"
make_package() {
  printf '#!/usr/bin/env bash\nprintf "kyvault %s\\n"\n' "$1" > "$TEST_DIR/pkg/kyvault"
  chmod +x "$TEST_DIR/pkg/kyvault"
  tar -czf "$TEST_DIR/package.tar.gz" -C "$TEST_DIR/pkg" kyvault
}
reset_old() {
  printf '#!/usr/bin/env bash\necho "kyvault 1.0.0"\n' > "$KYVAULT_INSTALL_DIR/kyvault"
  chmod +x "$KYVAULT_INSTALL_DIR/kyvault"
  ln -sf kyvault "$KYVAULT_INSTALL_DIR/ky"
  : > "$TEST_DIR/requests"
  : > "$TEST_DIR/side-effects"
}
run_installer() { bash "$ROOT/install.sh" > "$TEST_DIR/output" 2>&1; }
assert_old() {
  [ "$("$KYVAULT_INSTALL_DIR/kyvault" --version)" = 'kyvault 1.0.0' ]
  [ "$(readlink "$KYVAULT_INSTALL_DIR/ky")" = kyvault ]
  [ -f "$HOME/.keyring/module/kyvault/marker" ]
  [ "$(cat "$HOME/.keyring/master.key")" = 'keep vault' ]
  [ ! -s "$TEST_DIR/side-effects" ]
  [ -z "$(find "$KYVAULT_INSTALL_DIR" -name '.kyvault-update.*' -print)" ]
}
make_package 2.3.4
reset_old
export KYVAULT_VERSION=v2.3.4 KYVAULT_UPDATE_ONLY=1
run_installer
[ "$("$KYVAULT_INSTALL_DIR/kyvault" --version)" = 'kyvault 2.3.4' ]
[ "$(readlink "$KYVAULT_INSTALL_DIR/ky")" = kyvault ]
grep -q 'releases/download/v2.3.4/kyvault-x86_64-unknown-linux-gnu.tar.gz' "$TEST_DIR/requests"
! grep -q 'api.github.com\|releases/latest' "$TEST_DIR/requests"
[ ! -s "$TEST_DIR/side-effects" ]
echo 'PASS pinned target and symlink preservation'
reset_old
if FAIL_DOWNLOAD=1 run_installer; then exit 1; fi
assert_old
echo 'PASS failed download leaves old executable and vault intact'
make_package 9.9.9
reset_old
if run_installer; then exit 1; fi
assert_old
echo 'PASS mismatched version leaves old executable and vault intact'
printf 'not an archive\n' > "$TEST_DIR/package.tar.gz"
reset_old
if run_installer; then exit 1; fi
assert_old
echo 'PASS invalid archive leaves old executable intact'
reset_old
if KYVAULT_VERSION='v2.3.4/../../main' run_installer; then exit 1; fi
assert_old
[ ! -s "$TEST_DIR/requests" ]
echo 'PASS unsafe version rejected before download'
reset_old
if KYVAULT_VERSION='' run_installer; then exit 1; fi
assert_old
[ ! -s "$TEST_DIR/requests" ]
echo 'PASS update-only requires pinned version'
make_package 2.3.4
reset_old
KYVAULT_VERSION='' KYVAULT_UPDATE_ONLY=0 run_installer
grep -q 'api.github.com/repos/webkubor/kyvault/releases?per_page=100' "$TEST_DIR/requests"
grep -q 'releases/download/v2.3.4/' "$TEST_DIR/requests"
! grep -q 'releases/latest\|download/desktop' "$TEST_DIR/requests"
[ "$("$KYVAULT_INSTALL_DIR/kyvault" --version)" = 'kyvault 2.3.4' ]
echo 'PASS standalone latest selects CLI release over desktop'
