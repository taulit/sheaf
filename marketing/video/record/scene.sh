#!/usr/bin/env bash
# Runs inside the sandbox (see record.sh): prepares the desktop, starts Sheaf,
# records the screen while a virtual mouse and keyboard use the app.
set -euo pipefail
source "$SHEAF_REPO/scripts/lib/shell.sh"
here="$SHEAF_REPO/marketing/video/record"
export WAYLAND_DISPLAY=wayland-0
# shell-sandbox.sh starts the session with env -i, so derive the rest here.
DEMO_OUT="$SHEAF_REPO/marketing/video/public"
DEMO_SCHEME="$SHEAF_SCHEME"

# HiDPI: a 2880x1620 monitor at scale 2 gives sharp text when the video zooms in.
gdbus call --session --dest org.gnome.Mutter.DisplayConfig \
    --object-path /org/gnome/Mutter/DisplayConfig \
    --method org.gnome.Mutter.DisplayConfig.ApplyMonitorsConfig 2 1 \
    "[(0, 0, 2.0, 0, true, [('Meta-0', '2880x1620@60.000', @a{sv} {})])]" "@a{sv} {}" >/dev/null
gsettings set org.gnome.desktop.interface enable-animations true
gsettings set org.gnome.desktop.interface accent-color blue
gsettings set org.gnome.desktop.background picture-options zoom
gsettings set org.gnome.desktop.background picture-uri file:///usr/share/backgrounds/gnome/adwaita-l.jxl
gsettings set org.gnome.desktop.background picture-uri-dark file:///usr/share/backgrounds/gnome/adwaita-d.jxl

# The screencast service needs PipeWire.
pipewire >"$HOME/pipewire.log" 2>&1 &
sleep 0.5
wireplumber >"$HOME/wireplumber.log" 2>&1 &
sleep 1

"$here/seed-notes.sh"
mkdir -p "$HOME/Documents"
ln -sfn "$SHEAF_NOTES_DIR" "$HOME/Documents/Notes"
# A neutral prompt and window title for the terminal scene (no user or host name).
cat >"$HOME/.bashrc" <<'RC'
PS1='\[\e[1;34m\]\w\[\e[0m\] $ '
PROMPT_COMMAND='printf "\033]0;~/Documents/Notes\007"'
cd ~/Documents/Notes
RC

shell_eval "(globalThis.demo = $(cat "$here/input.js"), true)" >/dev/null

windows='global.display.list_all_windows()'
sheaf >"$HOME/sheaf.log" 2>&1 &
shell_wait_for "$windows.some(w => w.get_wm_class() === 'io.github.taulit.Sheaf')" 20
# The first start asks the GlobalShortcuts portal for Quick Note; close that dialog.
sleep 2
shell_eval "$windows.filter(w => w.get_wm_class() !== 'io.github.taulit.Sheaf').forEach(w => w.delete(global.get_current_time())), true" >/dev/null
# Changing the monitor or starting an app can bring up the overview; leave it.
shell_eval "Main.overview.hide(), true" >/dev/null
shell_wait_for "!Main.overview.visible && !Main.overview.animationInProgress" 10
shell_eval "demo.move(1300, 760, 10).then(() => true)" >/dev/null
sleep 2

marks="$DEMO_OUT/recording-$DEMO_SCHEME.json"
t0=0
mark() { printf '%s  "%s": %s' "$sep" "$1" "$(( $(date +%s%3N) - t0 ))" >>"$marks"; sep=$',\n'; }
run() { shell_eval "$1.then(() => true)" >/dev/null; }

shell_eval "Main.screenshotUI._screencastProxy.ScreencastAsync('$DEMO_OUT/recording-$DEMO_SCHEME.raw', {'draw-cursor': new GLib.Variant('b', true), 'framerate': new GLib.Variant('i', 30)}).then(r => r[1])" >/dev/null
t0=$(date +%s%3N)
# The red recording indicator is not part of the demo.
shell_eval "(Main.panel.statusArea.screenRecording.container.opacity = 0, true)" >/dev/null || true
for _ in 1 2 3; do
    shell_eval "Main.overview.hide(), true" >/dev/null
    shell_wait_for "!Main.overview.visible && !Main.overview.animationInProgress" 10
    sleep 0.5
done
shell_eval "Main.overview.visible" | grep -q false || { echo "overview still open" >&2; exit 1; }
printf '{\n' >"$marks"; sep=''

# Calm pacing: slow pointer moves, a pause after every action so the viewer sees
# what changed. The video holds one camera framing per scene.
mark gallery
sleep 2.5
shell_shot "$DEMO_OUT/stills/gallery-$DEMO_SCHEME.png"
run "demo.move(700, 300, 1400)"
sleep 1.0
run "demo.move(320, 342, 1400)"
sleep 0.8
mark open
run "demo.click(320, 342, 200)"
sleep 2.0
mark opened
shell_shot "$DEMO_OUT/stills/note-$DEMO_SCHEME.png"
sleep 0.8
mark check
run "demo.click(428, 295, 1200)"
sleep 1.8
mark type
run "demo.click(608, 326, 1000)"
run "demo.key('End')"
run "demo.key('Return')"
sleep 0.4
run "demo.type('Ask GNOME users for feedback', 110)"
sleep 2.5
shell_shot "$DEMO_OUT/stills/edit-$DEMO_SCHEME.png"
mark back
run "demo.key('Escape')"
sleep 2.2
mark search
run "demo.move(1000, 600, 800)"
run "demo.type('banan bred', 160)"
sleep 3.0
shell_shot "$DEMO_OUT/stills/search-$DEMO_SCHEME.png"
mark filter
run "demo.key('a', ['Control'])"
run "demo.type('#recipes has:checklist', 120)"
sleep 3.2
mark clear
run "demo.key('Escape')"
sleep 1.5
mark files
SHELL=/bin/bash kgx -- bash >"$HOME/kgx.log" 2>&1 &
shell_wait_for "$windows.some(w => w.get_wm_class() === 'org.gnome.Console')" 15
# Console shows its size for a moment after it opens; wait until that is gone.
sleep 2.4
mark ls
run "demo.type('ls *', 140)"
run "demo.key('Return')"
sleep 3.0
mark cat
run "demo.type('cat Work/Launch*', 110)"
run "demo.key('Return')"
sleep 4.5
shell_shot "$DEMO_OUT/stills/files-$DEMO_SCHEME.png"
mark end
sleep 0.5
shell_eval "Main.screenshotUI._screencastProxy.StopScreencastAsync().then(() => true)" >/dev/null
printf '\n}\n' >>"$marks"
sleep 1
