#!/bin/sh
set -eu
umask 077
exe=/Applications/Ky.app/Contents/MacOS/ky-desktop
support=${KY_APPLICATION_SUPPORT:-"$HOME/Library/Application Support"}
if [ ! -x "$exe" ]; then
  echo "请先安装含 ky-desktop 的 Ky.app。" >&2
  exit 1
fi
cd "$(dirname "$0")"
key=$(/usr/bin/plutil -extract key raw -o - extension/manifest.json)
id=$(echo "$key" | /usr/bin/base64 -D | /usr/bin/shasum -a 256 | cut -c1-32 | tr '0123456789abcdef' 'abcdefghijklmnop')
for browser in 'Google/Chrome' 'Microsoft Edge' 'BraveSoftware/Brave-Browser'; do
  dir="$support/$browser/NativeMessagingHosts"
  mkdir -p "$dir"
  tmp=$(mktemp "$dir/.ky-flash.XXXXXX")
  trap 'rm -f "$tmp"' EXIT
  cat > "$tmp" <<JSON
{
  "name": "com.kyvault.flash",
  "description": "Ky Flash Notes",
  "path": "$exe",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://$id/"]
}
JSON
  mv "$tmp" "$dir/com.kyvault.flash.json"
  echo "$dir/com.kyvault.flash.json"
done
