// SPDX-License-Identifier: GPL-3.0-or-later
import {initialState, reduce, screen, shouldLoad} from '../lib/state.js';
import {equal, test} from './harness.js';

const play = (...actions) => actions.reduce(reduce, initialState());
const note = {id: 'a', title: 'A', snippet: '', folder: '', modified: 1};

test('without Sheaf the screen is "missing" and nothing is loaded', () => {
  const state = initialState();
  equal(screen(state), 'missing');
  equal(shouldLoad(state, true), false);
});

test('Sheaf appearing makes the view load once it is on screen', () => {
  const state = play({type: 'installed', value: true});
  equal(screen(state), 'loading');
  equal(shouldLoad(state, false), false, 'not on screen');
  equal(shouldLoad(state, true), true);
});

test('loading, then the list; a change makes it stale again', () => {
  let state = play({type: 'installed', value: true}, {type: 'load'});
  equal(shouldLoad(state, true), false, 'already loading');
  state = reduce(state, {type: 'loaded', notes: [note]});
  equal(screen(state), 'list');
  equal(shouldLoad(state, true), false);
  state = reduce(state, {type: 'stale'});
  equal(shouldLoad(state, true), true);
  equal(screen(state), 'list', 'the old list stays until the new one is there');
});

test('an empty result is "empty"', () => {
  const ready = play({type: 'installed', value: true}, {type: 'load'}, {type: 'loaded', notes: []});
  equal(screen(ready), 'empty');
});

test('a failure is "failed" and is retried only when the view is shown again', () => {
  const failed = play({type: 'installed', value: true}, {type: 'load'}, {type: 'failed'});
  equal(screen(failed), 'failed');
  equal(shouldLoad(failed, true), false, 'no retry loop');
  equal(shouldLoad(reduce(failed, {type: 'shown'}), true), true);
  const ready = play({type: 'installed', value: true}, {type: 'load'}, {type: 'loaded', notes: []});
  equal(reduce(ready, {type: 'shown'}) === ready, true, 'showing a good view changes nothing');
});

test('a failure keeps the notes that were shown', () => {
  const state = play({type: 'installed', value: true}, {type: 'load'},
    {type: 'loaded', notes: [note]}, {type: 'load'}, {type: 'failed'});
  equal(screen(state), 'list');
});

test('uninstalling clears the notes, and installing again starts over', () => {
  let state = play({type: 'installed', value: true}, {type: 'load'}, {type: 'loaded', notes: [note]});
  state = reduce(state, {type: 'installed', value: false});
  equal(screen(state), 'missing');
  equal(state.notes, []);
  state = reduce(state, {type: 'installed', value: true});
  equal(shouldLoad(state, true), true);
});

test('repeating what is known changes nothing', () => {
  const state = play({type: 'installed', value: true});
  equal(reduce(state, {type: 'installed', value: true}) === state, true);
  equal(reduce(state, {type: 'stale'}) === state, true);
});
