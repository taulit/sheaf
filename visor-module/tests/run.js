// SPDX-License-Identifier: GPL-3.0-or-later
// Runs every tests/*.test.js with: gjs -m visor-module/tests/run.js
import Gio from 'gi://Gio';
import System from 'system';

import {run} from './harness.js';

const dir = Gio.File.new_for_uri(import.meta.url).get_parent();
const names = [];
const children = dir.enumerate_children('standard::name', Gio.FileQueryInfoFlags.NONE, null);
for (let info = children.next_file(null); info; info = children.next_file(null)) {
  if (info.get_name().endsWith('.test.js')) names.push(info.get_name());
}
for (const name of names.sort()) await import(`./${name}`);
System.exit(run() === 0 ? 0 : 1);
