import { describe, expect, it } from 'vitest';
import { covers, encloses, needsPassphrase, remaining, takeToken } from './model';

describe('grants', () => {
  it('match as ErisLogin matches them', () => {
    expect(covers('tasks:*', 'tasks:create')).toBe(true);
    expect(covers('tasks:read', 'tasks:create')).toBe(false);
    expect(covers('*', 'meta:pairing:approve')).toBe(true);
    expect(covers('*:read', 'meta:facets:read')).toBe(false);
    expect(covers('*:read', 'notes:read')).toBe(true);
    expect(covers('meta:*', 'meta:pairing:approve')).toBe(true);
    expect(covers('tasks:read', 'tasks:*')).toBe(false);
    expect(covers('tasks', 'tasks:read')).toBe(false);
    expect(covers('tasks:read:x', 'tasks:read')).toBe(false);
  });

  it('enclose when every child grant is covered', () => {
    expect(encloses(['notes:*'], ['notes:read', 'notes:delete'])).toBe(true);
    expect(encloses(['notes:read'], ['notes:read', 'notes:delete'])).toBe(false);
    expect(encloses(['*'], ['anything:at:all'])).toBe(true);
    expect(encloses([], [])).toBe(true);
  });

  it('need the passphrase only for what the device cannot approve itself', () => {
    expect(needsPassphrase(['notes:*'], ['notes:read'])).toBe(false);
    expect(needsPassphrase(['notes:*'], ['notes:read', 'tasks:read'])).toBe(true);
  });
});

describe('remaining', () => {
  it('counts down in minutes and seconds', () => {
    expect(remaining(1_000_600, 1_000_000)).toBe('10:00');
    expect(remaining(1_000_065, 1_000_000)).toBe('1:05');
    expect(remaining(1_000_000, 1_000_000)).toBe('0:00');
    expect(remaining(999_000, 1_000_000)).toBe('0:00');
  });
});

describe('takeToken', () => {
  it('takes the token out of a fragment and keeps the rest', () => {
    expect(takeToken('#token=abc')).toEqual({ token: 'abc', hash: '' });
    expect(takeToken('#token=abc&x=1')).toEqual({ token: 'abc', hash: '#x=1' });
    expect(takeToken('')).toEqual({ token: null, hash: '' });
  });
});
