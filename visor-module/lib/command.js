// SPDX-License-Identifier: GPL-3.0-or-later
// The command `note <text>` / `notiz <text>` of the search. No shell imports.

export const KEYWORDS = ['note', 'notiz'];

/** The text of a note from what was typed: one line, single spaces, no padding. */
export function noteText(raw) {
  return raw.replace(/\s+/g, ' ').trim();
}

/**
 * What Enter would do for the arguments of the command. Visor calls this on
 * every keystroke, so it only builds results; the work is in their activate().
 *
 * Without text the command opens the Quick Note window, with text it saves the
 * note to the Quick Notes folder (Ctrl+Enter also opens it in the app).
 *
 * @param {string} args
 * @param {{t: (s: string) => string, create: (text: string) => void,
 *          createAndOpen: (text: string) => void, quickNote: () => void}} actions
 */
export function commandResults(args, {t, create, createAndOpen, quickNote}) {
  const text = noteText(args);
  if (text === '') {
    return [{
      id: 'quick-note',
      title: t('Quick note'),
      subtitle: t('Opens a small window'),
      activate: () => quickNote(),
    }];
  }
  return [{
    id: 'new-note',
    title: text,
    subtitle: t('New note in Quick Notes'),
    hint: t('Ctrl+Enter opens it'),
    activate: () => create(text),
    alternate: () => createAndOpen(text),
  }];
}
