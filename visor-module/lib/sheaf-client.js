// SPDX-License-Identifier: GPL-3.0-or-later
// Client of io.github.taulit.Sheaf1, the D-Bus face of sheaf-searchd.
// Every call is asynchronous and has a timeout; nothing blocks the shell.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {mapRows} from './notes.js';

const SERVICE = 'io.github.taulit.Sheaf.SearchProvider';
const PATH = '/io/github/taulit/Sheaf/SearchProvider';
const IFACE = 'io.github.taulit.Sheaf1';

/**
 * The first call starts the service through D-Bus activation and may have to
 * build the index, so the timeout is generous. The user's own cancellable
 * ends a call earlier.
 */
const TIMEOUT_MS = 15000;

export class SheafClient {
  /**
   * @param {{onChanged: (ids: string[]) => void}} handlers `Changed` of the service
   */
  constructor({onChanged}) {
    this._onChanged = onChanged;
    this._connection = Gio.DBus.session;
    this._pending = new Set();
    // The bus follows the owner of the well-known name, so the subscription
    // survives the service exiting after five idle minutes and starting again.
    this._subscription = this._connection.signal_subscribe(
      SERVICE, IFACE, 'Changed', PATH, null, Gio.DBusSignalFlags.NONE,
      (_connection, _sender, _path, _iface, _signal, parameters) => {
        const [ids] = parameters.deepUnpack();
        this._onChanged(ids);
      });
  }

  /** Stops every running call and the subscription. */
  destroy() {
    this._connection.signal_unsubscribe(this._subscription);
    this._subscription = 0;
    for (const cancellable of this._pending) cancellable.cancel();
    this._pending.clear();
  }

  /** @returns {Promise<ReturnType<typeof mapRows>>} the best matches first */
  async search(query, limit, cancellable = null) {
    const reply = await this._call('Search', '(su)', [query, limit], '(a(ssssx))', cancellable);
    return mapRows(reply.deepUnpack()[0]);
  }

  /** @returns {Promise<ReturnType<typeof mapRows>>} the most recently changed notes first */
  async recent(limit, cancellable = null) {
    const reply = await this._call('Recent', '(u)', [limit], '(a(ssssx))', cancellable);
    return mapRows(reply.deepUnpack()[0]);
  }

  /**
   * Creates a note whose first line is the title.
   * @param {string} folder empty for the Quick Notes folder
   * @returns {Promise<string>} the id of the note
   */
  async create(text, folder = '') {
    const reply = await this._call('Create', '(ss)', [text, folder], '(s)', null);
    return reply.deepUnpack()[0];
  }

  /** Shows the note in the app, which D-Bus starts if needed. */
  async open(id) {
    await this._call('Open', '(s)', [id], '()', null);
  }

  /** Opens the Quick Note window of the app. */
  async quickNote() {
    await this._call('QuickNote', null, null, '()', null);
  }

  _call(method, signature, values, replyType, external) {
    const cancellable = new Gio.Cancellable();
    this._pending.add(cancellable);
    const stop = external?.connect(() => cancellable.cancel());
    const parameters = signature === null ? null : new GLib.Variant(signature, values);
    return new Promise((resolve, reject) => {
      this._connection.call(
        SERVICE, PATH, IFACE, method, parameters, new GLib.VariantType(replyType),
        Gio.DBusCallFlags.NONE, TIMEOUT_MS, cancellable,
        (connection, result) => {
          this._pending.delete(cancellable);
          if (stop) external.disconnect(stop);
          try {
            resolve(connection.call_finish(result));
          } catch (e) {
            reject(e);
          }
        });
    });
  }
}

/** True for the error of a call that was cancelled on purpose. */
export function isCancelled(error) {
  return error.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED) ?? false;
}
