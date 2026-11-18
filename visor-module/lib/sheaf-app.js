// SPDX-License-Identifier: GPL-3.0-or-later
// The Sheaf app as the desktop knows it: is it installed, open it, find it on Flathub.
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';

const DESKTOP_ID = 'io.github.taulit.Sheaf.desktop';
const FLATHUB_URL = 'https://flathub.org/apps/io.github.taulit.Sheaf';

/** The id Visor needs to hide Sheaf's own results in the GNOME search. */
export const SEARCH_PROVIDER_ID = DESKTOP_ID;

export class SheafApp {
  /** @param {() => void} onInstalledChanged called when apps are installed or removed */
  constructor(onInstalledChanged) {
    this._system = Shell.AppSystem.get_default();
    this._system.connectObject('installed-changed', () => onInstalledChanged(), this);
  }

  destroy() {
    this._system.disconnectObject(this);
  }

  get installed() {
    return this._system.lookup_app(DESKTOP_ID) !== null;
  }

  /** Shows the window of the app, or starts the app. */
  open() {
    this._system.lookup_app(DESKTOP_ID)?.activate();
  }

  /** Opens the page of the app on Flathub in the browser. */
  openFlathub() {
    Gio.AppInfo.launch_default_for_uri_async(
      FLATHUB_URL, global.create_app_launch_context(0, -1), null,
      (_source, result) => {
        try {
          Gio.AppInfo.launch_default_for_uri_finish(result);
        } catch (e) {
          console.warn(`Visor: Notes: cannot open ${FLATHUB_URL}: ${e.message}`);
        }
      });
  }
}
