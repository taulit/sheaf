// Test-only helper, loaded by scripts/shell-sandbox.sh inside the sandboxed
// headless shell. It exports io.github.taulit.Sheaf.Debug so the test scripts
// can drive the shell and take screenshots; the shell's own Eval and Screenshot
// D-Bus methods are closed to outside tools. Sheaf itself ships no debug code.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const IFACE = `
<node>
  <interface name="io.github.taulit.Sheaf.Debug">
    <method name="Eval">
      <arg type="s" direction="in" name="script"/>
      <arg type="b" direction="out" name="success"/>
      <arg type="s" direction="out" name="result"/>
    </method>
    <method name="Screenshot">
      <arg type="s" direction="in" name="path"/>
    </method>
  </interface>
</node>`;

export default class SheafTestHelper extends Extension {
  enable() {
    this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
    this._dbus.export(Gio.DBus.session, '/io/github/taulit/Sheaf/Debug');
    this._nameId = Gio.bus_own_name_on_connection(
      Gio.DBus.session,
      'io.github.taulit.Sheaf.Debug',
      Gio.BusNameOwnerFlags.NONE,
      null,
      null,
    );
  }

  disable() {
    Gio.bus_unown_name(this._nameId);
    this._dbus.unexport();
    this._dbus = null;
  }

  // The script is an expression; a returned promise is awaited.
  async EvalAsync([script], invocation) {
    try {
      const run = new Function(
        'Main',
        'St',
        'Clutter',
        'Gio',
        'GLib',
        'Shell',
        'Meta',
        `return (${script});`,
      );
      const value = await run(Main, St, Clutter, Gio, GLib, Shell, Meta);
      const json = JSON.stringify(value);
      invocation.return_value(new GLib.Variant('(bs)', [true, json ?? '']));
    } catch (e) {
      invocation.return_value(new GLib.Variant('(bs)', [false, `${e}\n${e.stack ?? ''}`]));
    }
  }

  ScreenshotAsync([path], invocation) {
    const file = Gio.File.new_for_path(path);
    const stream = file.replace(null, false, Gio.FileCreateFlags.NONE, null);
    const shooter = new Shell.Screenshot();
    shooter.screenshot(false, stream, (source, result) => {
      try {
        source.screenshot_finish(result);
        stream.close(null);
        invocation.return_value(null);
      } catch (e) {
        invocation.return_gerror(e);
      }
    });
  }
}
