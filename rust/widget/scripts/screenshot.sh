#!/bin/sh
# Runs the built widget on sample data inside a virtual display and saves what it shows:
#   docs/widget.png  the card
#   docs/hover.png   the card with the "working sessions" hover card open (needs xdotool)
#
#   sh scripts/screenshot.sh [fixture.json]
#
# Needs: cargo build (in src-tauri), xvfb, imagemagick; xdotool for the hover card.
# Nothing real is touched: HOME points at a throwaway folder and the data is the fixture.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
fixture=${1:-$here/fixtures/sample-full.json}
home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
mkdir -p "$here/docs"

HOME="$home" LOCALAPPDATA="$home" TMPDIR="$home" AIUT_FAKE_DATA="$fixture" AIUT_SHOTS="$here/docs" \
  xvfb-run -a -s "-screen 0 1280x1300x24" sh -c '
    "$0/src-tauri/target/debug/AIUsageTracker" &
    app=$!
    sleep ${AIUT_WAIT:-8}
    import -window root "$AIUT_SHOTS/screen.png"
    if command -v xdotool >/dev/null; then
      # the "working" label sits right of the title, near the top of the card
      eval "$(xdotool search --name "^AI Usage$" getwindowgeometry --shell | head -n 6)"
      xdotool mousemove $((X + 105)) $((Y + 20)); sleep 0.3
      xdotool mousemove $((X + 110)) $((Y + 21)); sleep 2
      import -window root "$AIUT_SHOTS/screen-hover.png"
    fi
    kill $app
  ' "$here"

# Cut the screenshots down to the windows (everything that is not the empty black screen).
convert "$here/docs/screen.png" -trim +repage "$here/docs/widget.png"
rm "$here/docs/screen.png"
if [ -f "$here/docs/screen-hover.png" ]; then
  convert "$here/docs/screen-hover.png" -trim +repage "$here/docs/hover.png"
  rm "$here/docs/screen-hover.png"
fi
echo "saved $here/docs/widget.png"
