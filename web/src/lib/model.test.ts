import { describe, expect, it } from 'vitest';
import { ago, headline, newPassphraseProblem, refusal, remaining, short, takeFromFragment, toRemove } from './model';
import type { ErisDb, Prompt } from './types';

const prompt = (over: Partial<Prompt>): Prompt => ({
  id: 'r1',
  kind: 'app',
  via: 'agent',
  name: 'Notes',
  key: 'ab'.repeat(32),
  service: 'notes',
  grants: ['notes:read'],
  serves: false,
  program: '/usr/bin/notes',
  pid: 42,
  package: null,
  certificate: null,
  fingerprint: null,
  passphrase: false,
  becomes_erisdb: false,
  expires: 100,
  busy: false,
  ...over,
});

const erisdb: ErisDb = {
  app: 'cd'.repeat(32),
  note: 'ef'.repeat(32),
  name: 'ErisDB',
  device: 'server',
  approved_at: 100,
  reachable: true,
};

describe('remaining', () => {
  it('counts down in minutes and seconds', () => {
    expect(remaining(1_000_600, 1_000_000)).toBe('10:00');
    expect(remaining(1_000_065, 1_000_000)).toBe('1:05');
    expect(remaining(999_000, 1_000_000)).toBe('0:00');
  });
});

describe('ago', () => {
  it('says how long ago as a person does', () => {
    expect(ago(1000, 1030)).toBe('just now');
    expect(ago(1000, 1000 + 60)).toBe('1 minute ago');
    expect(ago(1000, 1000 + 7200)).toBe('2 hours ago');
    expect(ago(1000, 1000 + 3 * 86_400)).toBe('3 days ago');
  });
});

describe('takeFromFragment', () => {
  it('takes one parameter out of a fragment and keeps the rest', () => {
    expect(takeFromFragment('#token=abc', 'token')).toEqual({ value: 'abc', hash: '' });
    expect(takeFromFragment('#token=abc&join=x', 'token')).toEqual({ value: 'abc', hash: '#join=x' });
    expect(takeFromFragment('', 'token')).toEqual({ value: null, hash: '' });
  });
});

describe('short', () => {
  it('keeps the ends of a long key', () => {
    expect(short('0123456789abcdef0123')).toBe('01234567…ef0123');
    expect(short('abc')).toBe('abc');
  });
});

describe('headline', () => {
  it('says what a request asks', () => {
    expect(headline(prompt({ kind: 'device', name: 'phone', service: null }), null).title).toBe(
      'phone asks to join your devices',
    );
    expect(headline(prompt({}), null).title).toBe('Notes asks to use notes');
    expect(headline(prompt({ service: 'erisdb', passphrase: true }), erisdb).title).toBe('Notes asks to use ErisDB');
    expect(headline(prompt({ via: 'ticket' }), null).detail).toContain('by ticket');
  });

  it('says an ErisDB asking to serve wants to be the mesh’s, and what it replaces', () => {
    const server = prompt({ name: 'ErisDB 2', service: 'erisdb', serves: true, becomes_erisdb: true, grants: [] });
    expect(headline(server, null).title).toBe('ErisDB 2 asks to be your ErisDB');
    expect(headline(server, null).detail).not.toContain('in place of');
    expect(headline(server, erisdb).detail).toContain('in place of ErisDB on server');
  });
});

describe('refusal', () => {
  it('says why in words', () => {
    expect(refusal({ error: 'wrong_passphrase' })).toBe('That is not the passphrase.');
    expect(refusal({ error: 'locked', retry_after: 125 })).toBe(
      'Too many wrong passphrases from this device. Try again in 2:05.',
    );
    expect(refusal({ error: 'no_erisdb', message: 'this mesh has no ErisDB yet' })).toBe('this mesh has no ErisDB yet');
    expect(refusal({})).toBe('ErisAuth refused.');
  });
});

describe('toRemove', () => {
  it('carries the device and every device it added the person kept ticked', () => {
    const removal = {
      device: { id: 'phone', name: 'phone' },
      also: [
        { id: 'tablet', name: 'tablet' },
        { id: 'watch', name: 'watch' },
      ],
    };
    expect(toRemove(removal, new Set())).toEqual(['phone', 'tablet', 'watch']);
    expect(toRemove(removal, new Set(['watch']))).toEqual(['phone', 'tablet']);
  });
});

describe('newPassphraseProblem', () => {
  it('asks for eight characters, twice the same', () => {
    expect(newPassphraseProblem('short', '')).toBe('At least 8 characters.');
    expect(newPassphraseProblem('long enough', 'long enougj')).toBe('The two differ.');
    expect(newPassphraseProblem('long enough', 'long enough')).toBe('');
    expect(newPassphraseProblem('', '')).toBe('');
  });
});
