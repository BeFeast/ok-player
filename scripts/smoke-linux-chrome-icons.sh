#!/usr/bin/env bash
# X11/Xvfb gate for the chrome's icon set (#731).
#
# The report was not a missing icon. The desktop's GTK icon theme was MacTahoe, a
# third-party macOS-style set that carries its own `system-search-symbolic` - a
# large ring with a thin handle - so the name resolved and the theme's design
# appeared in our Settings rail. Shipping copies of the standard names behind the
# theme would not have changed a pixel: on GTK 4.22 a path registered with
# `gtk_icon_theme_add_resource_path` is consulted only after every theme in the
# selected theme's inheritance chain.
#
# So the claim under test here is not "the names resolve" but "the names resolve
# to *our* files while a hostile theme is selected". Two runs:
#
#   hostile   - an icon theme that carries every standard name our chrome used to
#               ask for, each drawn as an unmistakable marker. Every chrome icon
#               must still come out of the binary, and the shell must never ask
#               the theme for a standard name in the first place.
#   stripped  - the clean-container case: an icon theme that carries nothing at
#               all. Every chrome icon must still resolve, so nothing falls
#               through to GTK's image-missing marker.
#
# The evidence is the shell's own: under OKP_DEBUG_INTERACTIONS it resolves its
# whole inventory and reports the file behind every name, and GTK_DEBUG=icontheme
# records every lookup it makes. Screenshots are captured too, because a record
# can describe a widget that draws nothing.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ISOLATED_XVFB="$ROOT/scripts/run-linux-isolated-xvfb-session.sh"
ISOLATED_DBUS="$ROOT/scripts/run-linux-isolated-dbus-session.sh"

# Every standard freedesktop name the chrome used to ask for. The hostile theme
# below carries all of them, and none of them may be looked up any more.
STANDARD_NAMES=(
  audio-volume-high-symbolic audio-volume-low-symbolic audio-volume-muted-symbolic
  audio-x-generic-symbolic camera-photo-symbolic dialog-error-symbolic
  dialog-information-symbolic dialog-warning-symbolic document-open-recent-symbolic
  document-open-symbolic document-send-symbolic edit-clear-all-symbolic
  edit-clear-symbolic edit-copy-symbolic edit-find-symbolic emblem-system-symbolic
  go-down-symbolic go-next-symbolic go-previous-symbolic go-up-symbolic
  image-x-generic-symbolic insert-link-symbolic list-add-symbolic
  list-drag-handle-symbolic list-remove-symbolic media-playback-pause-symbolic
  media-playback-start-symbolic media-seek-forward-symbolic
  media-skip-backward-symbolic media-skip-forward-symbolic
  media-view-subtitles-symbolic network-server-symbolic object-select-symbolic
  pan-down-symbolic process-working-symbolic system-search-symbolic
  user-bookmarks-symbolic
  video-x-generic-symbolic view-fullscreen-symbolic view-list-symbolic
  view-more-symbolic view-pin-symbolic view-restore-symbolic window-close-symbolic
)

OUR_RESOURCE_PREFIX="resource:///com/befeast/okplayer/icons/"

if [[ "${1:-}" == "--themes" ]]; then
  # Write the two icon themes. Kept in one place so the inner run and any manual
  # reproduction build the same thing.
  themes="${2:?missing theme root}"
  rm -rf "$themes"
  hostile="$themes/OkpHostileTheme"
  mkdir -p "$hostile/status/24" "$hostile/actions/24"
  {
    printf '[Icon Theme]\nName=OkpHostileTheme\nInherits=hicolor\n'
    printf 'Directories=status/24,actions/24\n\n'
    printf '[status/24]\nSize=24\nContext=Status\nType=Fixed\n\n'
    printf '[actions/24]\nSize=24\nContext=Actions\nType=Fixed\n'
  } >"$hostile/index.theme"
  # A marker no OK Player icon could be mistaken for, with the hardcoded fill
  # MacTahoe uses, so a theme win is obvious on a screenshot as well as in a log.
  marker='<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">'
  marker+='<rect x="2" y="2" width="20" height="20" fill="#f2f2f2"/>'
  marker+='<rect x="7" y="7" width="10" height="10" fill="#101010"/></svg>'
  for name in "${STANDARD_NAMES[@]}"; do
    printf '%s' "$marker" >"$hostile/status/24/$name.svg"
    printf '%s' "$marker" >"$hostile/actions/24/$name.svg"
  done
  # ... and the same names again under our prefix would be a theme claiming our
  # namespace, which is out of scope: no shipped theme defines `okp-*`.

  stripped="$themes/OkpStrippedTheme"
  mkdir -p "$stripped/scalable/actions"
  {
    printf '[Icon Theme]\nName=OkpStrippedTheme\nComment=Carries no icons\n'
    printf 'Directories=scalable/actions\n\n'
    printf '[scalable/actions]\nSize=16\nMinSize=8\nMaxSize=512\n'
    printf 'Type=Scalable\nContext=Actions\n'
  } >"$stripped/index.theme"
  exit 0
fi

if [[ "${1:-}" == "--inner" ]]; then
  shift
  BINARY="${1:?missing binary}"
  OUT_DIR="${2:?missing output directory}"
  THEME="${3:?missing icon theme name}"

  export GDK_BACKEND=x11
  export GTK_USE_PORTAL=0
  export NO_AT_BRIDGE=1
  export XDG_SESSION_TYPE=x11
  export XDG_CURRENT_DESKTOP=KDE
  export XDG_STATE_HOME="$OUT_DIR/state"
  export XDG_CONFIG_HOME="$OUT_DIR/config"
  export XDG_CACHE_HOME="$OUT_DIR/cache"
  export XDG_DATA_HOME="$OUT_DIR/data"
  # The stripped run is the clean-container case, and a container that has no
  # icon theme installed also has no /usr/share/icons to inherit one from. Taking
  # the system data directories away is stronger than any container image: the
  # only icons left anywhere are GTK's own builtins and ours.
  if [[ "$THEME" == OkpStrippedTheme ]]; then
    mkdir -p "$OUT_DIR/empty-datadir"
    export XDG_DATA_DIRS="$OUT_DIR/empty-datadir"
  fi
  export LIBGL_ALWAYS_SOFTWARE=1
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
  mkdir -p "$XDG_STATE_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME"

  # The themes go where only this run can see them, and the run selects one the
  # way a desktop does. XDG_CURRENT_DESKTOP is KDE because that is the reporter's
  # desktop and the settings.ini path is how KDE hands GTK its icon theme.
  "$0" --themes "$XDG_DATA_HOME/icons"
  for version in gtk-3.0 gtk-4.0; do
    mkdir -p "$XDG_CONFIG_HOME/$version"
    printf '[Settings]\ngtk-icon-theme-name=%s\n' "$THEME" \
      >"$XDG_CONFIG_HOME/$version/settings.ini"
  done

  xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
  wm_pid=$!
  app_pid=""
  cleanup() {
    [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
    kill "$wm_pid" 2>/dev/null || true
  }
  trap cleanup EXIT

  for _ in $(seq 1 100); do
    xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' && break
    sleep 0.05
  done
  xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' || {
    echo "xfwm4 did not become ready" >&2
    exit 75
  }

  # Settings opens on Integration, which puts all three ways an icon gets
  # requested on one surface: the rail's own `gtk::Image` icons, the rail search
  # field whose magnifier and clear button GTK builds for itself (the widget from
  # the report), and the history-retention `GtkDropDown`, whose arrow takes its
  # icon from CSS. The assertions below only reach as far as what a run draws, so
  # what it draws is part of the gate - see the negative controls on the pull
  # request, one per mechanism.
  GTK_DEBUG=icontheme \
  OKP_DEBUG_INTERACTIONS=1 \
  OKP_OPEN_SETTINGS_ON_STARTUP=1 \
  OKP_OPEN_SETTINGS_PAGE_ON_STARTUP=integration \
  OKP_SETTINGS_COLOR_SCHEME=light \
  OKP_DISABLE_MPRIS=1 \
  OKP_SKIP_UPDATE_CHECK=1 \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  timeout 90s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!

  settings_id=""
  for _ in $(seq 1 200); do
    if xdotool search --onlyvisible --name '^Settings$' >"$OUT_DIR/settings.ids" 2>/dev/null \
      && [[ -s "$OUT_DIR/settings.ids" ]]; then
      while IFS= read -r candidate; do
        info="$(xwininfo -id "$candidate" 2>/dev/null || true)"
        width="$(awk '/Width:/ { print $2; exit }' <<<"$info")"
        state="$(awk -F': ' '/Map State:/ { print $2; exit }' <<<"$info")"
        if [[ "${width:-0}" -gt 1 && "$state" == "IsViewable" ]]; then
          settings_id="$candidate"
          break 2
        fi
      done <"$OUT_DIR/settings.ids"
    fi
    sleep 0.25
  done
  [[ -n "$settings_id" ]] || {
    echo "the Settings window never appeared" >&2
    tail -40 "$OUT_DIR/app.log" >&2
    exit 1
  }
  xdotool windowactivate --sync "$settings_id" >/dev/null 2>&1 || true
  # The inventory is reported at startup; the rail's own icons resolve as the
  # page draws. Give the first frame time to settle before reading either back.
  sleep 4
  import -window "$settings_id" "$OUT_DIR/settings.png"
  player_id="$(xdotool search --onlyvisible --name '^OK Player' 2>/dev/null | head -1 || true)"
  [[ -n "$player_id" ]] && import -window "$player_id" "$OUT_DIR/player.png"

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  log="$OUT_DIR/app.log"

  # 0. The run really did select the theme under test. Without this the two
  #    assertions below could both pass against a desktop that has no theme at
  #    all, which is the one case this gate is not about.
  reported_theme="$(awk -F'name=' '
    index($0, "interaction: chrome-icon-theme name=") == 1 {
      split($2, parts, " "); value = parts[1]
    } END { print value }' "$log")"
  [[ "$reported_theme" == "$THEME" ]] || {
    echo "the shell reports icon theme '$reported_theme' rather than '$THEME'" >&2
    exit 1
  }

  # 1. Every icon in the inventory came out of the binary. A host theme that had
  #    taken one of our names would show up here as a file:// source.
  awk 'index($0, "interaction: chrome-icon name=") == 1' "$log" >"$OUT_DIR/inventory.txt"
  inventory_count="$(wc -l <"$OUT_DIR/inventory.txt")"
  declared_count="$(awk -F'count=' '
    index($0, "interaction: chrome-icon-theme name=") == 1 { value = $2 } END { print value }' "$log")"
  (( inventory_count >= 44 )) || {
    echo "the shell reported only $inventory_count chrome icons" >&2
    exit 1
  }
  [[ "$inventory_count" == "$declared_count" ]] || {
    echo "the shell declared $declared_count chrome icons and reported $inventory_count" >&2
    exit 1
  }
  if awk -v prefix="$OUR_RESOURCE_PREFIX" '
      { source = ""
        for (i = 1; i <= NF; i++) { if (index($i, "source=") == 1) { source = substr($i, 8) } }
        if (index(source, prefix) != 1) { print; found = 1 } }
      END { exit !found }' "$OUT_DIR/inventory.txt" >"$OUT_DIR/foreign-icons.txt"; then
    echo "chrome icons that did not come out of the binary:" >&2
    cat "$OUT_DIR/foreign-icons.txt" >&2
    exit 1
  fi

  # 2. And the shell never asked the theme for a standard name. Matching on the
  #    whole field, because every shipped name has a standard one inside it.
  grep -oE 'looking up icon [a-zA-Z0-9._+-]+' "$log" \
    | awk '{ print $4 }' | sort -u >"$OUT_DIR/looked-up.txt"
  printf '%s\n' "${STANDARD_NAMES[@]}" | sort -u >"$OUT_DIR/standard-names.txt"
  comm -12 "$OUT_DIR/standard-names.txt" "$OUT_DIR/looked-up.txt" >"$OUT_DIR/leaked.txt"
  if [[ -s "$OUT_DIR/leaked.txt" ]]; then
    echo "the shell still asks the host theme for standard icon names:" >&2
    cat "$OUT_DIR/leaked.txt" >&2
    exit 1
  fi

  # 3. Nothing fell through to GTK's marker for a name that resolved to nothing.
  if grep -q 'No icon found in .* for: okp-' "$log"; then
    echo "shipped icon names that failed to resolve:" >&2
    grep 'No icon found in .* for: okp-' "$log" >&2
    exit 1
  fi
  if grep -qE 'looking up icon image-missing' "$log"; then
    echo "the shell fell back to image-missing" >&2
    exit 1
  fi

  # 4. The surface drew. A screenshot of a blank window would satisfy everything
  #    above, so the captured image has to carry more than one colour.
  colours="$(magick "$OUT_DIR/settings.png" -format '%k' info:)"
  (( colours > 16 )) || {
    echo "the Settings screenshot carries only $colours colours" >&2
    exit 1
  }

  if awk '/panicked at|fatal runtime error|core dumped/ { print; found = 1 } END { exit !found }' \
      "$log" >&2; then
    echo "the chrome-icon smoke observed a fatal process diagnostic" >&2
    exit 1
  fi

  printf '%s\n' \
    "icon_theme=${THEME}" \
    "reported_icon_theme=${reported_theme}" \
    "chrome_icons_reported=${inventory_count}" \
    "chrome_icons_from_binary=${inventory_count}" \
    "standard_names_looked_up=0" \
    "unresolved_shipped_names=0" \
    "image_missing_fallbacks=0" \
    "settings_screenshot_colours=${colours}" \
    'fatal_diagnostics=absent' \
    'status=pass' >"$OUT_DIR/results.txt"
  exit 0
fi

BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-chrome-icons-smoke}"

for tool in xfwm4 xdotool xprop xwininfo import magick awk comm timeout; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

for theme in OkpHostileTheme OkpStrippedTheme; do
  run_dir="$OUT_DIR/$theme"
  mkdir -p "$run_dir"
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
    "$ISOLATED_XVFB" \
    "$run_dir/xvfb-evidence.txt" \
    "$run_dir/xvfb.log" \
    "-screen 0 1920x1080x24 -nolisten tcp" \
    "$ISOLATED_DBUS" \
    "$run_dir/dbus-evidence.txt" \
    "$0" --inner "$BINARY" "$run_dir" "$theme"
done

echo "Chrome-icon smoke passed. Results: $OUT_DIR/*/results.txt"
