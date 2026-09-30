export interface Identity {
  kind: 'iroh' | 'browser';
  key: string;
}

/** A request waiting for the person: an app on this device, or a device joining. */
export interface Approval {
  id: string;
  kind: 'app' | 'device';
  service: string;
  client: string;
  requested: string[];
  identity: Identity | null;
  /** For a device: the code both screens show, to compare. */
  fingerprint: string | null;
  /** For a device: what makes it an ErisAuth device, granted or refused whole. */
  essential: string[];
  pid: number | null;
  executable: string | null;
  needs_passphrase: boolean;
  expires: number;
}

export interface State {
  device: { name: string | null; joined: boolean; grants: string[] };
  approvals: Approval[];
  /** A join started on this page, until it ends. */
  joining: { fingerprint: string | null; error: string | null } | null;
}

/** A decision's answer: `{ok: true}`, or why not. */
export interface Outcome {
  ok?: true;
  error?: string;
  message?: string;
  retry_after?: number;
}

/** A ticket a new device joins with. */
export interface Added {
  id?: string;
  ticket?: string;
  /** The ticket as a QR code, an SVG document the daemon drew. */
  svg?: string;
  expires?: number;
  error?: string;
  message?: string;
}
