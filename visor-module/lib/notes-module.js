// SPDX-License-Identifier: GPL-3.0-or-later
// The "Notes" module of Visor: a dock view with a field for new notes and the
// most recent notes, search results, and the command `note <text>`.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import St from 'gi://St';

import {KEYWORDS, afterFailedCreate, commandResults, noteText} from './command.js';
import {displayTitle, noteToResult, oneLine} from './notes.js';
import {SEARCH_PROVIDER_ID, SheafApp} from './sheaf-app.js';
import {SheafClient, isCancelled} from './sheaf-client.js';
import {RECENT_LIMIT, initialState, reduce, screen, shouldLoad} from './state.js';
import {timeLabel} from './time.js';

// Visor takes the name of an icon of the theme; an extension cannot add icons to it. This
// one is a sheet with lines, like the symbolic icon of Sheaf.
const ICON = 'text-x-generic-symbolic';
const SEARCH_LIMIT = 5;

/** @param {import('resource:///org/gnome/shell/extensions/extension.js').Extension} extension */
export function createNotesModule(extension) {
  return new NotesModule(extension.gettext.bind(extension));
}

class NotesModule {
  constructor(gettext) {
    this.id = 'notes';
    this.name = gettext('Notes');
    this.icon = ICON;
    this.slot = 'dock';
    this.replacesSearchProvider = SEARCH_PROVIDER_ID;
    this.search = {
      id: 'notes',
      title: this.name,
      search: (query, cancellable) => this._search(query, cancellable),
    };
    this.commands = [{
      keyword: KEYWORDS,
      hint: gettext('note Buy milk'),
      resolve: args => this._resolve(args),
    }];

    this._ctx = null;
    this._client = null;
    this._app = null;
    this._state = null;
    this._loading = null;
    this._view = null;
  }

  enable(ctx) {
    this._ctx = ctx;
    this._state = initialState();
    this._client = new SheafClient({onChanged: () => this._dispatch({type: 'stale'})});
    this._app = new SheafApp(() => this._syncInstalled());
    this._syncInstalled();
  }

  disable() {
    this._loading?.cancel();
    this._loading = null;
    this._client?.destroy();
    this._client = null;
    this._app?.destroy();
    this._app = null;
    // Visor destroys the view itself.
    this._view = null;
    this._state = null;
    this._ctx = null;
  }

  /** Header buttons of the view; read every time it is shown. */
  get viewActions() {
    if (!this._ctx || !this._app?.installed) return [];
    const ctx = this._ctx;
    return [{
      label: ctx.t('Open in Notes'),
      style: 'ghost',
      run: () => this._openApp(),
    }, {
      label: ctx.t('Quick note'),
      icon: 'document-edit-symbolic',
      style: 'default',
      run: () => this._quickNote(),
    }];
  }

  buildView() {
    const {t} = this._ctx;
    const root = new St.BoxLayout({style_class: 'sheaf-notes-view', orientation: Clutter.Orientation.VERTICAL, x_expand: true});
    const entry = new St.Entry({
      style_class: 'sheaf-notes-entry', hint_text: t('New note …'), can_focus: true, x_expand: true,
    });
    // Before the text sees the key: since GNOME 51 its own bindings (Enter)
    // run ahead of key-press-event handlers connected to it.
    entry.connectObject('captured-event', (_entry, event) =>
      event.type() === Clutter.EventType.KEY_PRESS && global.stage.get_key_focus() === entry.clutter_text
        ? this._onKey(event)
        : Clutter.EVENT_PROPAGATE, this);
    const content = new St.BoxLayout({style_class: 'sheaf-notes-content', orientation: Clutter.Orientation.VERTICAL, x_expand: true});
    root.add_child(entry);
    root.add_child(content);
    this._view = {root, entry, content, error: null, saving: false};
    root.connectObject('notify::mapped', () => this._onMapped(), this);
    this._render();
    return root;
  }

  // --- state ---

  _dispatch(action) {
    if (!this._state) return;
    const before = this._state;
    this._state = reduce(before, action);
    if (screen(this._state) !== screen(before) || this._state.notes !== before.notes) this._render();
    this._loadIfNeeded();
  }

  _syncInstalled() {
    this._dispatch({type: 'installed', value: this._app.installed});
  }

  _onMapped() {
    if (!this._view?.root.mapped) return;
    // Times such as "5 min" grow old while the view is away.
    this._render();
    this._view.entry.grab_key_focus();
    this._dispatch({type: 'shown'});
  }

  _loadIfNeeded() {
    if (this._view && shouldLoad(this._state, this._view.root.mapped)) this._load();
  }

  async _load() {
    const cancellable = new Gio.Cancellable();
    this._loading = cancellable;
    this._dispatch({type: 'load'});
    try {
      const notes = await this._client.recent(RECENT_LIMIT, cancellable);
      this._dispatch({type: 'loaded', notes});
    } catch (e) {
      if (isCancelled(e)) return;
      console.warn(`Visor: Notes: cannot load the notes: ${e.message}`);
      this._dispatch({type: 'failed'});
    }
  }

  // --- view ---

  _render() {
    const view = this._view;
    if (!view) return;
    const {kit, t} = this._ctx;
    const current = screen(this._state);
    view.entry.visible = current !== 'missing';
    view.content.destroy_all_children();

    switch (current) {
    case 'missing':
      view.content.add_child(kit.empty(t('Notes is not installed')));
      view.content.add_child(kit.button({
        label: t('Get it on Flathub'), style: 'link', run: () => this._app.openFlathub(),
      }));
      break;
    case 'failed':
      view.content.add_child(kit.empty(t('Could not load the notes')));
      break;
    case 'empty':
    case 'list': {
      const section = kit.section({title: t('Recent')});
      if (current === 'empty') section.body.add_child(kit.empty(t('No notes yet')));
      const now = Date.now() / 1000;
      for (const note of this._state.notes) {
        section.body.add_child(kit.row({
          title: displayTitle(note, t),
          subtitle: oneLine(note.snippet),
          meta: timeLabel(note.modified, now, {t}),
          activate: () => this._openNote(note.id),
        }).actor);
      }
      view.content.add_child(section.actor);
      break;
    }
    default:
      break;
    }
  }

  _onKey(event) {
    const key = event.get_key_symbol();
    if (key !== Clutter.KEY_Return && key !== Clutter.KEY_KP_Enter && key !== Clutter.KEY_ISO_Enter)
      return Clutter.EVENT_PROPAGATE;
    const control = (event.get_state() & Clutter.ModifierType.CONTROL_MASK) !== 0;
    this._submit(control);
    return Clutter.EVENT_STOP;
  }

  async _submit(openAfterwards) {
    const view = this._view;
    const text = noteText(view.entry.text);
    if (text === '' || view.saving) return;
    const {kit, t} = this._ctx;
    const client = this._client;
    view.saving = true;
    view.error?.destroy();
    view.error = null;
    view.entry.text = '';
    try {
      const id = await client.create(text, '');
      if (openAfterwards && this._view === view) await this._openNote(id);
    } catch (e) {
      if (isCancelled(e)) return;
      console.warn(`Visor: Notes: cannot create the note: ${e.message}`);
      // The view is gone when the module was switched off meanwhile.
      if (this._view !== view) return;
      view.entry.text = afterFailedCreate(text, view.entry.text);
      view.error = kit.empty(t('Could not save the note'));
      view.root.insert_child_at_index(view.error, 1);
    } finally {
      view.saving = false;
    }
  }

  // --- actions ---

  async _openNote(id) {
    const ctx = this._ctx;
    const client = this._client;
    if (!ctx) return;
    ctx.close();
    try {
      await client.open(id);
    } catch (e) {
      this._failed('cannot open the note', e, ctx.t('Could not open Notes'));
    }
  }

  _openApp() {
    this._app?.open();
    this._ctx?.close();
  }

  async _quickNote() {
    const ctx = this._ctx;
    const client = this._client;
    if (!ctx) return;
    ctx.close();
    try {
      await client.quickNote();
    } catch (e) {
      this._failed('cannot open the quick note', e, ctx.t('Could not open Notes'));
    }
  }

  // The toast of the command: a chip in the peek state of the notch.
  async _saveQuickNote(text, openAfterwards) {
    const ctx = this._ctx;
    const client = this._client;
    try {
      const id = await client.create(text, '');
      ctx.peek.flash({icon: ICON, text: ctx.t('Saved to Quick Notes'), priority: 100});
      if (openAfterwards) await client.open(id);
    } catch (e) {
      this._failed('cannot create the note', e, ctx.t('Could not save the note'));
    }
  }

  _failed(what, error, message) {
    if (isCancelled(error)) return;
    console.warn(`Visor: Notes: ${what}: ${error.message}`);
    this._ctx?.peek.flash({icon: 'dialog-warning-symbolic', text: message, priority: 100});
  }

  // --- search and command ---

  async _search(query, cancellable) {
    if (!this._app?.installed) return [];
    const t = this._ctx.t;
    const notes = await this._client.search(query, SEARCH_LIMIT, cancellable);
    return notes.map(note => noteToResult(note, {t, open: id => this._openNote(id)}));
  }

  _resolve(args) {
    if (!this._ctx) return [];
    const {t} = this._ctx;
    if (!this._app.installed) {
      return [{
        id: 'missing',
        title: t('Notes is not installed'),
        subtitle: t('Get it on Flathub'),
        activate: () => this._app.openFlathub(),
      }];
    }
    return commandResults(args, {
      t,
      create: text => this._saveQuickNote(text, false),
      createAndOpen: text => this._saveQuickNote(text, true),
      quickNote: () => this._quickNote(),
    });
  }
}
