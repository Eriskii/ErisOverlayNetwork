/**
 * True when `grant` covers `required`, as ErisLogin decides it: segment by segment, `*` matches
 * one segment, and as a grant's last segment it matches every remaining one.
 */
export function covers(grant: string, required: string): boolean {
  const grants = grant.split(':');
  const wanted = required.split(':');
  for (let i = 0; i < grants.length; i++) {
    if (i >= wanted.length) return false;
    if (grants[i] === '*' && i === grants.length - 1) return true;
    if (grants[i] !== '*' && grants[i] !== wanted[i]) return false;
  }
  return wanted.length === grants.length;
}

/** True when every grant in `child` is covered by some grant in `parent`. */
export function encloses(parent: string[], child: string[]): boolean {
  return child.every((wanted) => parent.some((grant) => covers(grant, wanted)));
}

/** True when approving `granted` takes more than this device holds, so home and the passphrase. */
export function needsPassphrase(device: string[], granted: string[]): boolean {
  return !encloses(device, granted);
}

/** Time left until `expires` (Unix seconds) as m:ss. */
export function remaining(expires: number, now: number): string {
  const left = Math.max(0, Math.floor(expires - now));
  return `${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`;
}

/** The token handed over in a URL fragment, and the fragment without it. */
export function takeToken(hash: string): { token: string | null; hash: string } {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const token = params.get('token');
  params.delete('token');
  const rest = params.toString();
  return { token, hash: rest ? `#${rest}` : '' };
}

/** A ceiling as typed: grants separated by commas or spaces; nothing typed means everything. */
export function parseCeiling(typed: string): string[] {
  const grants = typed.split(/[\s,]+/).filter(Boolean);
  return grants.length ? grants : ['*'];
}
