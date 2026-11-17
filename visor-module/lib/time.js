// SPDX-License-Identifier: GPL-3.0-or-later
// Short time labels for the list of recent notes. No shell imports.

const DAY_MS = 86400000;

// en-CA formats as YYYY-MM-DD in any time zone.
function isoDay(date, timeZone) {
  return new Intl.DateTimeFormat('en-CA', {
    timeZone, year: 'numeric', month: '2-digit', day: '2-digit',
  }).format(date);
}

function dayNumber(date, timeZone) {
  return Date.parse(`${isoDay(date, timeZone)}T00:00:00Z`) / DAY_MS;
}

/**
 * "Now", "12 min", "14:05" (today), "Yesterday", a weekday (this week), "3 Nov" or "3 Nov 2025".
 *
 * @param {number} modified Unix seconds
 * @param {number} now Unix seconds
 * @param {{t: (s: string) => string, locale?: string, timeZone?: string}} options
 */
export function timeLabel(modified, now, {t, locale, timeZone}) {
  const age = now - modified;
  if (age < 60) return t('Now');
  if (age < 3600) return t('%d min').replace('%d', String(Math.floor(age / 60)));

  const date = new Date(modified * 1000);
  const today = new Date(now * 1000);
  const days = dayNumber(today, timeZone) - dayNumber(date, timeZone);
  const format = options => new Intl.DateTimeFormat(locale, {timeZone, ...options}).format(date);
  if (days <= 0) return format({hour: '2-digit', minute: '2-digit'});
  if (days === 1) return t('Yesterday');
  if (days < 7) return format({weekday: 'short'});
  const sameYear = isoDay(date, timeZone).slice(0, 4) === isoDay(today, timeZone).slice(0, 4);
  return sameYear
    ? format({day: 'numeric', month: 'short'})
    : format({day: 'numeric', month: 'short', year: 'numeric'});
}
