// SPDX-License-Identifier: GPL-3.0-or-later
import {timeLabel} from '../lib/time.js';
import {equal, test} from './harness.js';

const t = s => s;
const options = {t, locale: 'en-GB', timeZone: 'Europe/Berlin'};
const at = iso => Date.parse(iso) / 1000;
// Thursday 13 November 2025, 15:30 in Berlin.
const now = at('2025-11-13T14:30:00Z');

test('under a minute is Now, also for a clock that runs ahead', () => {
  equal(timeLabel(now - 20, now, options), 'Now');
  equal(timeLabel(now + 500, now, options), 'Now');
});

test('under an hour counts minutes', () => {
  equal(timeLabel(now - 60, now, options), '1 min');
  equal(timeLabel(now - 59 * 60, now, options), '59 min');
});

test('earlier today shows the time', () => {
  equal(timeLabel(at('2025-11-13T07:05:00Z'), now, options), '08:05');
});

test('what happened before midnight is yesterday, however short ago', () => {
  const justAfterMidnight = at('2025-11-13T23:30:00Z'); // 00:30 on the 14th in Berlin
  equal(timeLabel(at('2025-11-13T21:30:00Z'), justAfterMidnight, options), 'Yesterday');
});

test('yesterday and this week', () => {
  equal(timeLabel(at('2025-11-12T18:00:00Z'), now, options), 'Yesterday');
  equal(timeLabel(at('2025-11-10T10:00:00Z'), now, options), 'Mon');
});

test('older notes show the date, with the year when it differs', () => {
  equal(timeLabel(at('2025-11-03T10:00:00Z'), now, options), '3 Nov');
  equal(timeLabel(at('2024-12-24T10:00:00Z'), now, options), '24 Dec 2024');
});

test('labels follow the language', () => {
  const de = {t: s => ({Yesterday: 'Gestern'}[s] ?? s), locale: 'de', timeZone: 'Europe/Berlin'};
  equal(timeLabel(at('2025-11-12T18:00:00Z'), now, de), 'Gestern');
});
