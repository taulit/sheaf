// SPDX-License-Identifier: GPL-3.0-or-later
// Stand-in for the Sheaf app in the sandbox of scripts/visor-module-check.sh: owns
// io.github.taulit.Sheaf, serves org.freedesktop.Application and appends every call to
// the file named by SHEAF_FAKE_APP_LOG. D-Bus starts it like the real app.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const IFACE = `
<node>
  <interface name="org.freedesktop.Application">
    <method name="Activate"><arg type="a{sv}" direction="in"/></method>
    <method name="Open"><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/></method>
    <method name="ActivateAction">
      <arg type="s" direction="in"/><arg type="av" direction="in"/><arg type="a{sv}" direction="in"/>
    </method>
  </interface>
</node>`;

const log = Gio.File.new_for_path(GLib.getenv('SHEAF_FAKE_APP_LOG'));

function record(line) {
  const stream = log.append_to(Gio.FileCreateFlags.NONE, null);
  stream.write_all(`${line}\n`, null);
  stream.close(null);
}

const app = {
  Activate() {
    record('activate');
  },
  Open(uris) {
    record(`open ${uris.join(' ')}`);
  },
  ActivateAction(name, parameters) {
    record(`action ${name} ${JSON.stringify(parameters.map(p => p.recursiveUnpack()))}`);
  },
};

const exported = Gio.DBusExportedObject.wrapJSObject(IFACE, app);
const loop = new GLib.MainLoop(null, false);
Gio.bus_own_name(Gio.BusType.SESSION, 'io.github.taulit.Sheaf', Gio.BusNameOwnerFlags.NONE,
  connection => exported.export(connection, '/io/github/taulit/Sheaf'),
  null,
  () => loop.quit());
loop.run();
