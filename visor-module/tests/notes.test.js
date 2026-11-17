// SPDX-License-Identifier: GPL-3.0-or-later
import {afterFailedCreate, commandResults, noteText} from '../lib/command.js';
import {displayTitle, mapRows, noteToResult, oneLine} from '../lib/notes.js';
import {equal, test} from './harness.js';

const t = s => s;
const row = ['abc', 'Milch kaufen', 'Hafermilch\nund Butter', 'Schnellnotizen', 1700000000];

test('mapRows turns the D-Bus tuples into notes', () => {
  equal(mapRows([row]), [{
    id: 'abc', title: 'Milch kaufen', snippet: 'Hafermilch\nund Butter',
    folder: 'Schnellnotizen', modified: 1700000000,
  }]);
  equal(mapRows([]), []);
});

test('an empty title shows as Untitled', () => {
  const [note] = mapRows([['x', '  ', '', '', 0]]);
  equal(displayTitle(note, s => `[${s}]`), '[Untitled]');
  equal(displayTitle(mapRows([row])[0], t), 'Milch kaufen');
});

test('oneLine joins the lines of a snippet', () => {
  equal(oneLine('  a\n\n b\t c '), 'a b c');
});

test('a search result opens the note and hints the folder', () => {
  const opened = [];
  const result = noteToResult(mapRows([row])[0], {t, open: id => opened.push(id)});
  equal({...result, activate: typeof result.activate}, {
    id: 'abc', title: 'Milch kaufen', subtitle: 'Hafermilch und Butter',
    hint: 'Schnellnotizen', activate: 'function',
  });
  result.activate();
  equal(opened, ['abc']);
  equal(noteToResult({...mapRows([row])[0], folder: ''}, {t, open() {}}).hint, undefined);
});

test('noteText collapses whitespace', () => {
  equal(noteText('  Milch \n kaufen  '), 'Milch kaufen');
  equal(noteText('   '), '');
});

test('the command without text opens the quick note window', () => {
  const calls = [];
  const [result, ...rest] = commandResults('  ', {
    t, create: x => calls.push(['create', x]), createAndOpen: x => calls.push(['open', x]),
    quickNote: () => calls.push(['quick']),
  });
  equal(rest, []);
  equal(result.title, 'Quick note');
  result.activate();
  equal(calls, [['quick']]);
});

test('the command with text saves it, Ctrl+Enter also opens it', () => {
  const calls = [];
  const results = commandResults('Milch  kaufen', {
    t, create: x => calls.push(['create', x]), createAndOpen: x => calls.push(['open', x]),
    quickNote: () => calls.push(['quick']),
  });
  equal(results.length, 1);
  equal(results[0].title, 'Milch kaufen');
  equal(calls, [], 'resolving has no side effect');
  results[0].activate();
  results[0].alternate();
  equal(calls, [['create', 'Milch kaufen'], ['open', 'Milch kaufen']]);
});

test('a note that could not be created keeps its text and what was typed since', () => {
  equal(afterFailedCreate('Milch kaufen', ''), 'Milch kaufen');
  equal(afterFailedCreate('Milch kaufen', '  '), 'Milch kaufen');
  equal(afterFailedCreate('Milch kaufen', 'und Brot'), 'Milch kaufen und Brot');
});
