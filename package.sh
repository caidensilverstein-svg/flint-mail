#!/bin/zsh
set -e
cd "$(dirname "$0")"
ID="${FLINT_SIGN_ID:-$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/{print $2; exit}')}"
DEST="$HOME/Applications/Flint Mail.app"
(cd app && cargo build --release)
(cd helper && swiftc -O flint-cal.swift -o flint-cal -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker Info.plist)
pkill -x flint-mail || true
pkill -x flint-cal || true
sleep 1
mkdir -p "$DEST/Contents/MacOS"
cp "Flint Mail.app/Contents/Info.plist" "$DEST/Contents/Info.plist"
cp app/target/release/flint-mail "$DEST/Contents/MacOS/flint-mail"
cp helper/flint-cal "$DEST/Contents/MacOS/flint-cal"
xattr -rc "$DEST" 2>/dev/null || true
codesign --force --sign "$ID" "$DEST/Contents/MacOS/flint-cal"
codesign --force --sign "$ID" "$DEST"
codesign --verify "$DEST"
HELPER="$HOME/Applications/Flint Calendar Helper.app"
mkdir -p "$HELPER/Contents/MacOS"
cp helper/HelperInfo.plist "$HELPER/Contents/Info.plist"
cp helper/flint-cal "$HELPER/Contents/MacOS/flint-cal"
xattr -rc "$HELPER" 2>/dev/null || true
codesign --force --sign "$ID" "$HELPER"
codesign --verify "$HELPER"
open "$DEST"
