// SPDX-License-Identifier: GPL-3.0-or-later
// Connects a module to Visor as docs/api-v1.md of Visor describes it (section 3).
// The SDK helper connectVisor() of @taulit/visor-module does not exist at the tag
// api-v1, so this is the same steps by hand under the same name.
import {ExtensionState} from 'resource:///org/gnome/shell/misc/extensionUtils.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const VISOR_UUID = 'visor@taulit.github.io';
const API_VERSION = 1;

/**
 * Registers `module` while Visor is active and unregisters it when Visor goes or
 * the connection is destroyed. Without Visor, or with a Visor of another module
 * API, it does nothing and says so in the log once.
 *
 * @param {import('resource:///org/gnome/shell/extensions/extension.js').Extension} owner
 * @param {object} module a VisorModule
 */
export function connectVisor(owner, module) {
  return new VisorConnection(owner, module);
}

class VisorConnection {
  constructor(owner, module) {
    this._owner = owner;
    this._module = module;
    this._registeredWith = null;
    this._logged = false;
    Main.extensionManager.connectObject('extension-state-changed',
      (_manager, extension) => {
        if (extension.uuid === VISOR_UUID) this._sync();
      }, this);
    this._sync();
  }

  destroy() {
    Main.extensionManager.disconnectObject(this);
    if (this._registeredWith) this._api()?.unregister(this._module.id);
    this._registeredWith = null;
  }

  // The API of Visor while Visor is active and offers module API 1, else null.
  _api() {
    const visor = Main.extensionManager.lookup(VISOR_UUID);
    if (!visor) {
      this._say('Visor is not installed, doing nothing');
      return null;
    }
    if (visor.state !== ExtensionState.ACTIVE) return null;
    const api = visor.stateObj?.api;
    if (api?.version !== API_VERSION || typeof api.register !== 'function') {
      this._say(`this Visor has no module API ${API_VERSION}, doing nothing`);
      return null;
    }
    return api;
  }

  // Visor forgets every module when it is disabled, so a new ACTIVE needs a new register().
  _sync() {
    const api = this._api();
    if (api === null) {
      this._registeredWith = null;
    } else if (api !== this._registeredWith) {
      try {
        api.register(this._module, this._owner);
        this._registeredWith = api;
      } catch (e) {
        console.error(`${this._owner.uuid}: register failed: ${e.message}`);
      }
    }
  }

  _say(message) {
    if (this._logged) return;
    this._logged = true;
    console.log(`${this._owner.uuid}: ${message}`);
  }
}
