// SPDX-License-Identifier: GPL-3.0-or-later
// Notes as the Sheaf service returns them, and the search results made from them.
// No shell imports: the unit tests run this file with plain gjs.

/**
 * @param {Array<[string, string, string, string, number]>} rows `a(ssssx)` of
 *     io.github.taulit.Sheaf1: id, title, snippet, folder, modified (Unix seconds)
 * @returns {{id: string, title: string, snippet: string, folder: string, modified: number}[]}
 */
export function mapRows(rows) {
  return rows.map(([id, title, snippet, folder, modified]) => ({
    id,
    title,
    snippet,
    folder,
    modified: Number(modified),
  }));
}

/** A note without a title shows as "Untitled", like everywhere else in Sheaf. */
export function displayTitle(note, t) {
  return note.title.trim() === '' ? t('Untitled') : note.title;
}

/** Snippets can span lines; a row shows one. */
export function oneLine(text) {
  return text.replace(/\s+/g, ' ').trim();
}

/**
 * A search result of Visor for a note. Enter opens the note in the app.
 * The folder is the quiet hint on the right.
 */
export function noteToResult(note, {t, open}) {
  return {
    id: note.id,
    title: displayTitle(note, t),
    subtitle: oneLine(note.snippet),
    hint: note.folder === '' ? undefined : note.folder,
    activate: () => open(note.id),
  };
}
