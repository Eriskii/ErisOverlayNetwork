/** A request waiting for the person: an app asking for a note, or a new device asking to join. */
export interface Prompt {
  id: string;
  kind: 'app' | 'device';
  /** `agent`: an app on this device asked; `ticket`: a newcomer redeemed one of this device's tickets. */
  via: 'agent' | 'ticket';
  name: string;
  /** The key asking: the app's, or the new device's. */
  key: string;
  service: string | null;
  grants: string[];
  serves: boolean;
  /** What the operating system says is asking. */
  program: string | null;
  pid: number | null;
  package: string | null;
  certificate: string | null;
  /** For a request by ticket: the code both screens show. */
  fingerprint: string | null;
  /** Approving takes the ErisDB passphrase. */
  passphrase: boolean;
  /** Approving makes the app this mesh's ErisDB, in place of the one there is now. */
  becomes_erisdb: boolean;
  expires: number;
  busy: boolean;
}

export interface Device {
  id: string;
  name: string;
  added_by: string | null;
  added_by_name: string | null;
  added_at: number;
  this: boolean;
  /** Whether it answered when last asked; null before it has been asked. */
  reachable: boolean | null;
}

/** The mesh's ErisDB: of the apps approved to serve `erisdb`, the one this device has heard from. */
export interface ErisDb {
  app: string;
  note: string;
  name: string;
  /** The device that approved it. */
  device: string;
  approved_at: number;
  reachable: boolean;
}

/** A note this device issued: one app's approval. */
export interface Note {
  id: string;
  app: string;
  name: string;
  service: string;
  grants: string[];
  serves: boolean;
  program: string | null;
  at: number;
  revoked: boolean;
  counts: boolean;
  /** It is the mesh's ErisDB. */
  erisdb: boolean;
}

export interface Event {
  seq: number;
  at: number;
  text: string;
}

export interface OpenTicket {
  id: string;
  purpose: 'device' | 'app';
  status: 'pending' | 'requested';
  expires: number;
}

export interface State {
  version: number;
  device: { id: string; name: string | null };
  mesh: { id: string; member: boolean; devices: Device[]; erisdb: ErisDb | null } | null;
  joining: { fingerprint: string; host: string; error: string | null } | null;
  approvals: Prompt[];
  tickets: OpenTicket[];
  notes: Note[];
  events: Event[];
}

/** A command's answer: `{ok: true}` and what it carries, or why not. */
export interface Outcome {
  ok?: true;
  error?: string;
  message?: string;
  retry_after?: number;
  [key: string]: unknown;
}

/** A ticket this device shows. */
export interface Shown {
  id: string;
  purpose: 'device' | 'app';
  ticket: string;
  /** The ticket as a QR code, an SVG document the daemon drew. */
  svg: string;
  expires: number;
}

/** What removing a device offers to remove with it. */
export interface Removal {
  device: { id: string; name: string };
  also: { id: string; name: string | null }[];
}
