import type { ErisDb, Outcome, Prompt, Removal } from './types';

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

/** A service as a person reads it. */
export function serviceName(service: string | null): string {
  return service === 'erisdb' ? 'ErisDB' : (service ?? 'a service');
}

/** What a request says at the top of its card. */
export function headline(prompt: Prompt, erisdb: ErisDb | null): { title: string; detail: string } {
  if (prompt.kind === 'device') {
    return { title: `${prompt.name} asks to join your devices`, detail: 'A new device, with a ticket this one showed.' };
  }
  if (prompt.becomes_erisdb) {
    const replaces = erisdb ? `, in place of ${erisdb.name} on ${erisdb.device}` : '';
    return {
      title: `${prompt.name} asks to be your ErisDB`,
      detail: `Every device and app in the mesh will keep its data there${replaces}.`,
    };
  }
  const serving = prompt.serves ? ', and to serve it to other apps' : '';
  const via = prompt.via === 'ticket' ? 'An app with no ErisAuth beside it, by ticket.' : 'An app on this device.';
  return { title: `${prompt.name} asks to use ${serviceName(prompt.service)}${serving}`, detail: via };
}

/** Why a command did not go through, in words. */
export function refusal(outcome: Outcome): string {
  switch (outcome.error) {
    case 'needs_passphrase':
      return 'Approving this takes the ErisDB passphrase.';
    case 'wrong_passphrase':
      return 'That is not the passphrase.';
    case 'locked':
      return `Too many wrong passphrases from this device. Try again in ${remaining(outcome.retry_after ?? 0, 0)}.`;
    case 'not_asked':
      return 'Approve only what was asked for.';
    case 'no_such_request':
      return 'This request is gone: answered elsewhere, withdrawn, or out of time.';
    default:
      return outcome.message ?? outcome.error ?? 'ErisAuth refused.';
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
