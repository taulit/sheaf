// Test-only code that scripts/visor-module-check.sh evaluates inside the sandboxed shell:
// a virtual keyboard and pointer, and ways to look at Visor's panel. Visor's own
// objects (notch, content) are internals of Visor, not part of its module API;
// they are reached here only to drive and inspect the panel.
(() => {
  const seat = Clutter.get_default_backend().get_default_seat();
  const keyboard = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
  const pointer = seat.create_virtual_device(Clutter.InputDeviceType.POINTER_DEVICE);
  const now = () => GLib.get_monotonic_time();
  const wait = ms => new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => (resolve(true), false)));
  const visor = () => Main.extensionManager.lookup('visor@taulit.github.io').stateObj;
  const find = (cls, root = visor().notch.root) => {
    const found = [];
    const walk = actor => {
      if (actor.has_style_class_name?.(cls)) found.push(actor);
      for (const child of actor.get_children()) walk(child);
    };
    walk(root);
    return found;
  };
  const texts = actor => [actor.text, ...actor.get_children().flatMap(texts)].filter(Boolean);
  const text = (cls, root) => find(cls, root).find(a => a.mapped)?.text ?? '';
  const press = symbol => {
    keyboard.notify_keyval(now(), symbol, Clutter.KeyState.PRESSED);
    keyboard.notify_keyval(now(), symbol, Clutter.KeyState.RELEASED);
  };
  const click = async actor => {
    const [x, y] = actor.get_transformed_position();
    const [w, h] = actor.get_transformed_size();
    for (let i = 0; i < 10; i++) {
      pointer.notify_absolute_motion(now(), x + w / 2, y + h / 2);
      await wait(60);
      const [px, py] = global.get_pointer();
      if (Math.abs(px - x - w / 2) <= 1 && Math.abs(py - y - h / 2) <= 1) break;
    }
    pointer.notify_button(now(), Clutter.BUTTON_PRIMARY, Clutter.ButtonState.PRESSED);
    pointer.notify_button(now(), Clutter.BUTTON_PRIMARY, Clutter.ButtonState.RELEASED);
    await wait(200);
  };
  const view = () => find('sheaf-notes-view').find(a => a.mapped) ?? null;
  const sheet = () => visor().content.sheet;

  globalThis.sheafTest = {
    wait,
    find,
    view,
    click,
    // The ids of the modules Visor knows.
    modules: () => visor().content.registry.all().map(entry => entry.module.id),
    async open() {
      if (visor().notch.state !== 'open') visor().notch.open();
      await wait(200);
    },
    // Opens the panel on the view of the notes module with a real click on its dock icon.
    async showNotes() {
      await sheafTest.open();
      const dock = find('visor-dock-button').find(a => a.accessible_name === 'Notes' || a.accessible_name === 'Notizen');
      if (!dock) throw new Error('no dock icon of the notes module');
      await click(dock);
      return true;
    },
    // Title, quiet line and time of the rows of the view.
    rows: () => find('visor-row', view()).filter(a => a.mapped).map(row => [
      text('visor-row-title', row), text('visor-row-subtitle', row), text('visor-row-meta', row),
    ]),
    entry: () => find('sheaf-notes-entry', view())[0] ?? null,
    // Texts of the quiet lines (empty states, errors) in the view.
    quiet: () => find('visor-empty', view()).map(a => a.text),
    // Labels of the text buttons on screen (the header of the view, the body).
    buttons: () => find('visor-button').filter(a => a.mapped).map(a => a.label_actor?.text),
    async pressButton(label) {
      const button = find('visor-button').find(a => a.mapped && a.label_actor?.text === label);
      if (!button) throw new Error(`no button "${label}"`);
      await click(button);
      return true;
    },
    panelOpen: () => visor().notch.state === 'open',
    groupTitle: id => visor().content.finder.groups.find(group => group.id === id)?.label ?? null,
    // The actions in the header of the module view.
    viewActions: () => find('visor-view-actions').flatMap(a => a.get_children()).length,
    async type(string, settle = 150) {
      for (const char of string) {
        press(Clutter.unicode_to_keysym(char.codePointAt(0)));
        await wait(15);
      }
      await wait(settle);
    },
    async key(symbol, settle = 150) {
      press(symbol);
      await wait(settle);
    },
    async ctrlEnter(settle = 150) {
      keyboard.notify_keyval(now(), Clutter.KEY_Control_L, Clutter.KeyState.PRESSED);
      press(Clutter.KEY_Return);
      keyboard.notify_keyval(now(), Clutter.KEY_Control_L, Clutter.KeyState.RELEASED);
      await wait(settle);
    },
    // Types into the search entry of the panel, with the panel open and the entry empty.
    async ask(query, settle = 700) {
      await sheafTest.open();
      sheet().clearQuery();
      sheet().entry.grab_key_focus();
      await sheafTest.type(query, settle);
    },
    // The groups of the search list with the titles of their lines.
    results: () => visor().content.finder.groups.map(group => ({id: group.id, titles: group.items.map(i => i.title)})),
    // The chips of the notch, e.g. the confirmation of a quick note.
    chips: () => find('visor-chip').filter(a => a.mapped).map(a => texts(a).join(' ')),
    entryText: () => sheafTest.entry()?.text ?? null,
  };
  return true;
})()
