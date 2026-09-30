import { takeToken } from './model';
import type { Added, Outcome, State } from './types';

const KEY = 'erisauth.token';

/** The token for this daemon: handed over in the fragment, then kept for this tab. */
export function token(): string | null {
  const { token, hash } = takeToken(location.hash);
  if (token) {
    history.replaceState(history.state, '', `${location.pathname}${location.search}${hash}`);
    try {
      sessionStorage.setItem(KEY, token);
    } catch {
      // Kept in memory by the caller instead.
    }
    return token;
  }
  try {
    return sessionStorage.getItem(KEY);
  } catch {
    return null;
  }
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

export const approve = (bearer: string, id: string, granted: string[], passphrase?: string) =>
  call<Outcome>('POST', `./api/approvals/${encodeURIComponent(id)}/approve`, bearer, {
    granted,
    ...(passphrase ? { passphrase } : {}),
  });

export const deny = (bearer: string, id: string) =>
  call<Outcome>('POST', `./api/approvals/${encodeURIComponent(id)}/deny`, bearer);

export const add = (bearer: string) => call<Added>('POST', './api/devices', bearer);

export const join = (bearer: string, ticket: string, name: string, ceiling: string[]) =>
  call<Outcome>('POST', './api/join', bearer, { ticket, name, ceiling });

/** A ticket handed over in the fragment as `join=`, taken out of the address bar. */
export function ticketFromFragment(): string | null {
  const params = new URLSearchParams(location.hash.replace(/^#/, ''));
  const ticket = params.get('join');
  if (!ticket) return null;
  params.delete('join');
  const rest = params.toString();
  history.replaceState(history.state, '', `${location.pathname}${location.search}${rest ? `#${rest}` : ''}`);
  return ticket;
}
