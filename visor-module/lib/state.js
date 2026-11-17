// SPDX-License-Identifier: GPL-3.0-or-later
// What the dock view knows, as a reducer, and which screen that makes. No shell imports.

export const RECENT_LIMIT = 6;

export function initialState() {
  return {installed: false, status: 'idle', notes: [], stale: true};
}

/**
 * status: 'idle' (nothing asked yet), 'loading', 'ready' or 'failed'.
 * stale: the notes may have changed, load them the next time the view is on screen.
 * A failed load is not tried again at once but when the view is shown again ('shown').
 */
export function reduce(state, action) {
  switch (action.type) {
  case 'installed':
    if (action.value === state.installed) return state;
    return action.value
      ? {...state, installed: true, stale: true}
      : {installed: false, status: 'idle', notes: [], stale: true};
  case 'stale':
    return state.stale ? state : {...state, stale: true};
  case 'load':
    return {...state, status: 'loading', stale: false};
  case 'loaded':
    return {...state, status: 'ready', notes: action.notes};
  case 'failed':
    return {...state, status: 'failed', stale: false};
  case 'shown':
    return state.status === 'failed' ? {...state, stale: true} : state;
  default:
    return state;
  }
}

/** The view loads when it is on screen, Sheaf is there and what it shows may be out of date. */
export function shouldLoad(state, visible) {
  return visible && state.installed && state.stale && state.status !== 'loading';
}

/** 'missing', 'loading', 'failed', 'empty' or 'list'. */
export function screen(state) {
  if (!state.installed) return 'missing';
  if (state.notes.length > 0) return 'list';
  switch (state.status) {
  case 'ready': return 'empty';
  case 'failed': return 'failed';
  default: return 'loading';
  }
}
