import { describe, expect, it } from 'vitest';
import {
  ago,
  chosenGrants,
  grantKey,
  headline,
  listed,
  newPassphraseProblem,
  refusal,
  remaining,
  short,
  takeFromFragment,
  toRemove,
  your,
} from './model';
import type { Prompt } from './types';

const prompt = (over: Partial<Prompt>): Prompt => ({
  id: 'r1',
  kind: 'app',
  via: 'agent',
  name: 'Notes',
  key: 'ab'.repeat(32),
  uses: { 'notes/1': ['notes:read'] },
  serves: [],
  program: '/usr/bin/notes',
  pid: 42,
  package: null,
  certificate: null,
  fingerprint: null,
  passphrase: false,
  replaces: [],
  expires: 100,
  busy: false,
  ...over,
});

const database = { capability: 'database/1', single: true, passphrase: true };

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

describe('your and listed', () => {
  it('say capabilities and lists as a person does', () => {
    expect(your('database/1')).toBe('your database');
    expect(listed(['a'])).toBe('a');
    expect(listed(['a', 'b'])).toBe('a and b');
    expect(listed(['a', 'b', 'c'])).toBe('a, b and c');
  });
});

describe('headline', () => {
  it('says what a request asks', () => {
    expect(headline(prompt({ kind: 'device', name: 'phone', uses: {} })).title).toBe('phone asks to join your devices');
    expect(headline(prompt({})).title).toBe('Notes asks to use your notes');
    const tasks = prompt({ name: 'Tasks', uses: { 'database/1': ['tasks:*'], 'files/1': [] }, passphrase: true });
    expect(headline(tasks).title).toBe('Tasks asks to use your database and your files');
    const files = prompt({ name: 'ErisFiles', uses: { 'files/1': ['files:read'] }, serves: [{ capability: 'files/1' }] });
    expect(headline(files).title).toBe('ErisFiles asks to use your files, and to serve files');
    expect(headline(prompt({ via: 'ticket' })).detail).toContain('by ticket');
  });

  it('says a server of something a mesh has one of wants to be it, and what it replaces', () => {
    const server = prompt({ name: 'ErisDB 2', uses: {}, serves: [database] });
    expect(headline(server).title).toBe('ErisDB 2 asks to be your database');
    expect(headline(server).detail).not.toContain('in place of');
    const replacing = prompt({ name: 'ErisDB 2', uses: {}, serves: [database], replaces: [{ capability: 'database/1', name: 'ErisDB' }] });
    expect(headline(replacing).detail).toContain('in place of ErisDB');
  });
});

describe('chosenGrants', () => {
  it('keeps what is ticked in each capability, and leaves out one whose grants all went', () => {
    const asked = prompt({ uses: { 'database/1': ['tasks:read', 'tasks:write'], 'files/1': ['files:read'], 'notes/1': [] } });
    const ticked = new Set([grantKey('database/1', 'tasks:read')]);
    expect(chosenGrants(asked, ticked)).toEqual({ 'database/1': ['tasks:read'], 'notes/1': [] });
  });
});

describe('refusal', () => {
  it('says why in words', () => {
    expect(refusal({ error: 'wrong_passphrase' })).toBe('That is not the passphrase.');
    expect(refusal({ error: 'locked', retry_after: 125 })).toBe(
      'Too many wrong passphrases from this device. Try again in 2:05.',
    );
    expect(refusal({ error: 'no_server', message: 'nothing in this mesh serves database/1 yet' })).toBe(
      'nothing in this mesh serves database/1 yet',
    );
    expect(refusal({})).toBe('eon refused.');
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
