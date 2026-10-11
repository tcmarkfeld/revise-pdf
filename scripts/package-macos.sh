#!/bin/sh
# Builds dist/Revise.app. Options:
#   --dmg      also build dist/Revise.dmg
#   --install  copy the app to /Applications and register it with Launch
#              Services (so Finder's Open With offers it for PDFs)
#   --release  sign with a Developer ID (hardened runtime), build the dmg and
#              notarize + staple it, ready to publish. Uses $SIGN_IDENTITY
#              (default: the first "Developer ID Application" identity) and
#              the notarytool keychain profile $NOTARY_PROFILE (default
#              revise-notary, see `xcrun notarytool store-credentials`), or
#              $APPLE_ID / $APPLE_TEAM_ID / $APPLE_APP_PASSWORD when set (CI).
# The bundle is self-contained: PDFium ships in Contents/Frameworks, where the
# app looks for it first. Without --release it is signed ad-hoc, which is
# enough to run on this Mac only.
set -eu
release=false
case " $* " in *" --release "*) release=true ;; esac
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

[ -f vendor/pdfium/lib/libpdfium.dylib ] || scripts/fetch-pdfium.sh
cargo build --release -p app

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=dist/Revise.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
cp target/release/revise "$app/Contents/MacOS/revise"
cp vendor/pdfium/lib/libpdfium.dylib "$app/Contents/Frameworks/"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Revise</string>
  <key>CFBundleDisplayName</key><string>Revise</string>
  <key>CFBundleIdentifier</key><string>com.tcmarkfeld.revise</string>
  <key>CFBundleExecutable</key><string>revise</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleIconFile</key><string>Revise</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>PDF Document</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSItemContentTypes</key><array><string>com.adobe.pdf</string></array>
      <!-- Alternate: offered in Open With without replacing the default viewer. -->
      <key>LSHandlerRank</key><string>Alternate</string>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>Revise Document</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSItemContentTypes</key><array><string>com.tcmarkfeld.revise.document</string></array>
      <key>LSHandlerRank</key><string>Owner</string>
    </dict>
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>com.tcmarkfeld.revise.document</string>
      <key>UTTypeDescription</key><string>Revise Document</string>
      <key>UTTypeConformsTo</key><array><string>public.data</string></array>
      <key>UTTypeTagSpecification</key>
      <dict><key>public.filename-extension</key><array><string>revise</string><string>reflow</string></array></dict>
    </dict>
  </array>
</dict>
</plist>
EOF

# Icon: scripts/icon.png (1024×1024, artwork within the centre 824×824).
if [ -f scripts/icon.png ]; then
  iconset=$(mktemp -d)/Revise.iconset
  mkdir -p "$iconset"
  for s in 16 32 128 256 512; do
    sips -z $s $s scripts/icon.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) scripts/icon.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$app/Contents/Resources/Revise.icns"
fi

if $release; then
  identity=${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)}
  [ -n "$identity" ] || { echo "No Developer ID Application identity found" >&2; exit 1; }
  sign() { codesign --force --options runtime --timestamp --sign "$identity" "$@"; }
else
  sign() { codesign --force --sign - "$@"; }
fi
sign "$app/Contents/Frameworks/libpdfium.dylib"
sign "$app"
codesign --verify --strict "$app"
echo "Built $app"

lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
# Keep only the installed copy registered, so Open With launches it.
"$lsregister" -u "$app" >/dev/null 2>&1 || true

for arg in "$@"; do
  if [ "$arg" = "--install" ]; then
    # Remove the copy installed under the app's old name.
    if [ -d /Applications/Reflow.app ]; then
      "$lsregister" -u /Applications/Reflow.app >/dev/null 2>&1 || true
      rm -rf /Applications/Reflow.app
    fi
    rm -rf /Applications/Revise.app
    cp -R "$app" /Applications/
    "$lsregister" -f /Applications/Revise.app
    echo "Installed /Applications/Revise.app"
  fi
done

if $release || echo " $* " | grep -q -- " --dmg "; then
  rm -f dist/Revise.dmg
  staging=$(mktemp -d)
  cp -R "$app" "$staging/"
  ln -s /Applications "$staging/Applications"
  hdiutil create -volname Revise -srcfolder "$staging" -ov -format UDZO dist/Revise.dmg >/dev/null
  echo "Built dist/Revise.dmg"
fi

if $release; then
  sign dist/Revise.dmg
  if [ -n "${APPLE_ID:-}" ]; then
    xcrun notarytool submit dist/Revise.dmg --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
  else
    xcrun notarytool submit dist/Revise.dmg --keychain-profile "${NOTARY_PROFILE:-revise-notary}" --wait
  fi
  xcrun stapler staple dist/Revise.dmg
  spctl --assess --type open --context context:primary-signature -v dist/Revise.dmg
  echo "Notarized dist/Revise.dmg (version $version)"
  shasum -a 256 dist/Revise.dmg
fi
