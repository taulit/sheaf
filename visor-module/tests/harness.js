// SPDX-License-Identifier: GPL-3.0-or-later
// A small test harness for plain gjs: no shell, no npm.

const tests = [];

export function test(name, body) {
  tests.push({name, body});
}

function show(value) {
  return JSON.stringify(value, (_key, v) => (v === undefined ? '(undefined)' : v));
}

export function equal(actual, expected, what = '') {
  if (show(actual) !== show(expected))
    throw new Error(`${what}\n  expected ${show(expected)}\n  got      ${show(actual)}`);
}

/** Runs every collected test; returns the number of failures. */
export function run() {
  let failed = 0;
  for (const {name, body} of tests) {
    try {
      body();
      print(`ok    ${name}`);
    } catch (e) {
      failed++;
      print(`FAIL  ${name}: ${e.message}`);
    }
  }
  print(`${tests.length - failed} of ${tests.length} passed`);
  return failed;
}
