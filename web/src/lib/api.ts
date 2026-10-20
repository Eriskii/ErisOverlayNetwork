import { takeFromFragment } from './model';
import type { Outcome, State } from './types';

const KEY = 'erisauth.token';

/** The token for this daemon: handed over in the fragment, then kept for this tab. */
export function token(): string | null {
  const { value, hash } = takeFromFragment(location.hash, 'token');
  if (value) {
    history.replaceState(history.state, '', `${location.pathname}${location.search}${hash}`);
    try {
      sessionStorage.setItem(KEY, value);
    } catch {
      // Kept in memory by the caller instead.
    }
    return value;
  }
  try {
    return sessionStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/** A ticket handed over in the fragment as `join=`, taken out of the address bar. */
export function ticketFromFragment(): string | null {
  const { value, hash } = takeFromFragment(location.hash, 'join');
  if (value) history.replaceState(history.state, '', `${location.pathname}${location.search}${hash}`);
  return value;
}

export class Unauthorized extends Error {}

async function call<T>(verb: 'GET' | 'POST', path: string, bearer: string, body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method: verb,
    credentials: 'omit',
    headers: {
      authorization: `Bearer ${bearer}`,
      ...(body === undefined ? {} : { 'content-type': 'application/json' }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (response.status === 401) throw new Unauthorized();
  const value = await response.json().catch(() => null);
  if (!response.ok && !value) throw new Error(`HTTP ${response.status}`);
  return value as T;
}

export const state = (bearer: string) => call<State>('GET', './api/state', bearer);

/** One command, answered as the daemon's socket answers it. */
export const command = <T = Outcome>(bearer: string, message: { op: string; [key: string]: unknown }) =>
  call<T>('POST', './api/command', bearer, message);
