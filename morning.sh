#!/bin/zsh
# Run this on the Mac after copying the folder back. Builds, installs and opens Flint Mail.
# The running app is only replaced if the build succeeds, and the previous version is kept.
set -e
cd "$(dirname "$0")"
DEST="$HOME/Applications/Flint Mail.app"
PREV="$HOME/Applications/Flint Mail (previous).app"
echo "Building Flint Mail. This takes a few minutes the first time."
(cd app && cargo build --release)
if [ -d "$DEST" ]; then
  rm -rf "$PREV"
  cp -R "$DEST" "$PREV"
  echo "Kept the old version at: $PREV"
fi
./package.sh
echo ""
echo "Flint Mail is open. Two things to do:"
echo "  1. Settings > AI: paste your Groq key and press Test."
echo "  2. Click the menu bar item and try the new rows."
