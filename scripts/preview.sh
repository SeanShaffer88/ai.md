#!/bin/zsh
# Replace the running hand-test build (dist-preview) with target/release. It
# runs as dist-preview/ai.md.app (so the menu bar says ai.md) on the
# saved preview library, or ~/Documents/ai.md. The app initializes the
# bundled 开始指南 on first launch, just like an installed build.
# Only this checkout's own binaries are stopped:
# pkill matches this checkout's absolute path. Log: /tmp/ai-md-preview.log
set -e
cd "$(dirname "$0")/.."
preview_library="${AI_MD_HOME:-${QUIET_MD_HOME:-$(cat dist-preview/library-path 2>/dev/null || printf '%s' "$HOME/Documents/ai.md")}}"
# An empty path would put the library in the current directory.
[ -n "$preview_library" ] || { echo "preview.sh: no preview library" >&2; exit 1; }
export AI_MD_HOME="$preview_library"
app="$PWD/dist-preview/ai.md.app"
bin="$app/Contents/MacOS/ai_md"
pkill -f "^$bin" || true
sleep 1
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp assets/branding/app-icon.png "$app/Contents/Resources/app-icon.png"
icon_tmp=$(mktemp -d)
iconset="$icon_tmp/app-icon.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/branding/app-icon.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) assets/branding/app-icon.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/app-icon.icns"
rm -rf "$icon_tmp"
cp target/release/ai_md "$bin.bin"
# Finder launches must use the same library as this script.
printf '#!/bin/zsh\nexport AI_MD_HOME=%q\nexec "${0:A:h}/ai_md.bin" "$@"\n' "$preview_library" > "$bin"
chmod +x "$bin"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>ai.md</string>
  <key>CFBundleDisplayName</key><string>ai.md</string>
  <key>CFBundleIdentifier</key><string>local.aimd.preview</string>
  <key>CFBundleExecutable</key><string>ai_md</string>
  <key>CFBundleIconFile</key><string>app-icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSDocumentsFolderUsageDescription</key><string>ai.md 把文档库放在「文稿」里的 ai.md 文件夹。</string>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>Markdown</string>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key><array><string>net.daringfireball.markdown</string></array>
    <key>CFBundleTypeExtensions</key><array><string>md</string><string>markdown</string><string>mdown</string></array>
  </dict></array>
</dict></plist>
PLIST
codesign --force --sign - "$app" >/dev/null 2>&1 || true
(AI_MD_HOME="$AI_MD_HOME" nohup "$bin" >/tmp/ai-md-preview.log 2>&1 &)
sleep 2; pgrep -fl "^$bin"
