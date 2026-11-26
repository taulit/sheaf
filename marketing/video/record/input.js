// Virtual pointer and keyboard inside the sandboxed shell, installed as
// globalThis.demo through the test helper's Eval. Coordinates are logical pixels.
(() => {
  const seat = global.stage.context.get_backend().get_default_seat();
  const kb = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
  const ptr = seat.create_virtual_device(Clutter.InputDeviceType.POINTER_DEVICE);
  const now = () => GLib.get_monotonic_time();
  const sleep = (ms) => new Promise((resolve) => {
    GLib.timeout_add(GLib.PRIORITY_DEFAULT, Math.max(1, Math.round(ms)), () => {
      resolve();
      return GLib.SOURCE_REMOVE;
    });
  });
  const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
  const KEYS = {
    Return: Clutter.KEY_Return, Escape: Clutter.KEY_Escape, BackSpace: Clutter.KEY_BackSpace,
    Tab: Clutter.KEY_Tab, Control: Clutter.KEY_Control_L, Shift: Clutter.KEY_Shift_L,
    End: Clutter.KEY_End, Home: Clutter.KEY_Home, Down: Clutter.KEY_Down, Up: Clutter.KEY_Up,
  };
  const keyval = (k) => KEYS[k] ?? Clutter.unicode_to_keysym(k.codePointAt(0));
  const press = (kv) => kb.notify_keyval(now(), kv, Clutter.KeyState.PRESSED);
  const release = (kv) => kb.notify_keyval(now(), kv, Clutter.KeyState.RELEASED);
  return {
    sleep,
    async move(x, y, ms = 600) {
      const [x0, y0] = global.get_pointer();
      const steps = Math.max(1, Math.round(ms / 16));
      for (let i = 1; i <= steps; i++) {
        const t = ease(i / steps);
        ptr.notify_absolute_motion(now(), x0 + (x - x0) * t, y0 + (y - y0) * t);
        await sleep(16);
      }
    },
    async click(x, y, ms = 600) {
      await this.move(x, y, ms);
      await sleep(120);
      ptr.notify_button(now(), Clutter.BUTTON_PRIMARY, Clutter.ButtonState.PRESSED);
      await sleep(70);
      ptr.notify_button(now(), Clutter.BUTTON_PRIMARY, Clutter.ButtonState.RELEASED);
    },
    async scroll(dy, steps = 20) {
      for (let i = 0; i < steps; i++) {
        ptr.notify_scroll_continuous(now(), 0, dy / steps, Clutter.ScrollSource.FINGER, Clutter.ScrollFinishFlags.NONE);
        await sleep(16);
      }
      ptr.notify_scroll_continuous(now(), 0, 0, Clutter.ScrollSource.FINGER, Clutter.ScrollFinishFlags.VERTICAL);
    },
    async key(name, mods = []) {
      for (const m of mods) press(keyval(m));
      press(keyval(name));
      await sleep(40);
      release(keyval(name));
      for (const m of [...mods].reverse()) release(keyval(m));
      await sleep(60);
    },
    async type(text, ms = 70) {
      for (const ch of text) {
        if (ch.charCodeAt(0) === 10) await this.key('Return');
        else {
          const kv = keyval(ch);
          press(kv);
          await sleep(25);
          release(kv);
        }
        await sleep(ms * (0.6 + Math.random() * 0.8));
      }
    },
  };
})()
