import type { Outcome, Prompt, Removal } from './types';

/** Time left until `expires` (Unix seconds) as m:ss. */
export function remaining(expires: number, now: number): string {
  const left = Math.max(0, Math.floor(expires - now));
  return `${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`;
}

/** How long ago `at` (Unix seconds) was, as a person says it. */
export function ago(at: number, now: number): string {
  const secs = Math.max(0, Math.floor(now - at));
  if (secs < 60) return 'just now';
  const [count, unit] = secs < 3600 ? [secs / 60, 'minute'] : secs < 86_400 ? [secs / 3600, 'hour'] : [secs / 86_400, 'day'];
  const n = Math.floor(count);
  return `${n} ${unit}${n === 1 ? '' : 's'} ago`;
}

/** One parameter handed over in a URL fragment, and the fragment without it. */
export function takeFromFragment(hash: string, name: string): { value: string | null; hash: string } {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const value = params.get(name);
  params.delete(name);
  const rest = params.toString();
  return { value, hash: rest ? `#${rest}` : '' };
}

/** The start and end of a long key or digest. */
export function short(text: string, head = 8, tail = 6): string {
  return text.length <= head + tail + 1 ? text : `${text.slice(0, head)}…${text.slice(-tail)}`;
}

/** A capability as a person says it: `database/1` is "your database". */
export function your(capability: string): string {
  return `your ${capability.split('/')[0]}`;
}

/** Things listed as a person lists them: "a", "a and b", "a, b and c". */
export function listed(items: string[]): string {
  return items.length < 2 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

/** What a request says at the top of its card. */
export function headline(prompt: Prompt): { title: string; detail: string } {
  if (prompt.kind === 'device') {
    return { title: `${prompt.name} asks to join your devices`, detail: 'A new device, with a ticket this one showed.' };
  }
  const single = prompt.serves.filter((s) => s.single).map((s) => s.capability);
  if (single.length) {
    const replaces = prompt.replaces.length ? `, in place of ${listed(prompt.replaces.map((r) => r.name))}` : '';
    return {
      title: `${prompt.name} asks to be ${listed(single.map(your))}`,
      detail: `Every app that uses ${listed(single.map(your))} will use it${replaces}.`,
    };
  }
  const used = Object.keys(prompt.uses);
  const served = prompt.serves.map((s) => s.capability.split('/')[0]);
  const via = prompt.via === 'ticket' ? 'An app with no eon beside it, by ticket.' : 'An app on this device.';
  if (!used.length) return { title: `${prompt.name} asks to serve ${listed(served)}`, detail: via };
  const serving = served.length ? `, and to serve ${listed(served)}` : '';
  return { title: `${prompt.name} asks to use ${listed(used.map(your))}${serving}`, detail: via };
}

/** One grant in one capability, as a card keeps track of what is ticked. */
export const grantKey = (capability: string, grant: string) => `${capability} ${grant}`;

/** The grants ticked in each capability, as `decide` takes them. A capability whose grants were all
 * unticked is left out; one that asked for none stays. */
export function chosenGrants(prompt: Prompt, ticked: Set<string>): Record<string, string[]> {
  const chosen: Record<string, string[]> = {};
  for (const [capability, grants] of Object.entries(prompt.uses)) {
    const kept = grants.filter((g) => ticked.has(grantKey(capability, g)));
    if (kept.length || !grants.length) chosen[capability] = kept;
  }
  return chosen;
}

/** Why a command did not go through, in words. */
export function refusal(outcome: Outcome): string {
  switch (outcome.error) {
    case 'needs_passphrase':
      return 'Approving this takes a passphrase.';
    case 'wrong_passphrase':
      return 'That is not the passphrase.';
    case 'locked':
      return `Too many wrong passphrases from this device. Try again in ${remaining(outcome.retry_after ?? 0, 0)}.`;
    case 'not_asked':
      return 'Approve only what was asked for.';
    case 'no_such_request':
      return 'This request is gone: answered elsewhere, withdrawn, or out of time.';
    default:
      return outcome.message ?? outcome.error ?? 'eon refused.';
  }
}

/** The devices one removal carries: the device, and every device it added that the person did not untick. */
export function toRemove(removal: Removal, kept: Set<string>): string[] {
  return [removal.device.id, ...removal.also.map((d) => d.id).filter((id) => !kept.has(id))];
}

/** What is wrong with a new passphrase as typed twice; empty when nothing is. */
export function newPassphraseProblem(next: string, again: string): string {
  if (next.length > 0 && [...next].length < 8) return 'At least 8 characters.';
  if (again.length > 0 && again !== next) return 'The two differ.';
  return '';
}
