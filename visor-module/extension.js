// SPDX-License-Identifier: GPL-3.0-or-later
// Visor: Notes. Plugs the Sheaf notes app into Visor, the notch of GNOME Shell.
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {createNotesModule} from './lib/notes-module.js';
import {connectVisor} from './lib/visor.js';

export default class SheafVisorModule extends Extension {
  enable() {
    this._visor = connectVisor(this, createNotesModule(this));
  }

  disable() {
    this._visor?.destroy();
    this._visor = null;
  }
}
