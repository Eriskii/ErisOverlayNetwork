/** A request waiting for the person: an app asking for a note, or a new device asking to join. */
export interface Prompt {
  id: string;
  kind: 'app' | 'device';
  /** `agent`: an app on this device asked; `ticket`: a newcomer redeemed one of this device's tickets. */
  via: 'agent' | 'ticket';
  name: string;
  /** The key asking: the app's, or the new device's. */
  key: string;
  /** What the app uses, `database/1`, `files/1`, with the grants it asks for in each. */
  uses: Record<string, string[]>;
  /** What the app serves. */
  serves: Served[];
  /** What the operating system says is asking. */
  program: string | null;
  pid: number | null;
  package: string | null;
  certificate: string | null;
  /** For a request by ticket: the code both screens show. */
  fingerprint: string | null;
  /** Approving takes a passphrase: the app uses something whose server takes one. */
  passphrase: boolean;
  /** Approving makes the app the single server of these, in place of the apps serving them now. */
  replaces: { capability: string; name: string }[];
  expires: number;
  busy: boolean;
}

/** Serving a capability: `single` when a mesh has one server of it, `passphrase` when apps using it take one. */
export interface Served {
  capability: string;
  single?: boolean;
  passphrase?: boolean;
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

/** What the mesh's apps serve: one capability, and the apps serving it, newest first. For a single one, the first is in use. */
export interface Server {
  capability: string;
  single: boolean;
  passphrase: boolean;
  apps: {
    app: string;
    note: string;
    name: string;
    /** The device that approved it. */
    device: string;
    approved_at: number;
    /** Whether it answered when last asked; null when it is not asked. */
    reachable: boolean | null;
  }[];
}

/** A note this device issued: one app's approval. */
export interface Note {
  id: string;
  app: string;
  name: string;
  uses: Record<string, string[]>;
  serves: Served[];
  program: string | null;
  at: number;
  revoked: boolean;
  counts: boolean;
}

/** The ticket this device shows to be added, while it is in no mesh, and what is offered on it. */
export interface Offer {
  name: string;
  ticket: string;
  /** The ticket as a QR code, an SVG document the daemon drew. */
  svg: string;
  expires: number;
  status: 'showing' | 'offered' | 'accepted' | 'joined';
  /** The device offering to add this one. */
  by?: string;
  /** Every device in the mesh offered. */
  devices?: string[];
}

/** A device this one is adding by the ticket it shows. */
export interface Inviting {
  name: string;
  status: 'waiting' | 'added' | 'declined' | 'failed';
  error: string | null;
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
  device: { id: string; name: string | null; hostname: string | null };
  mesh: { id: string; member: boolean; devices: Device[]; servers: Server[] } | null;
  joining: { fingerprint: string; host: string; error: string | null } | null;
  offer: Offer | null;
  inviting: Inviting | null;
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
  /** For a device: the short code a person types in the ticket's place. */
  code?: string;
  expires: number;
}

/** What removing a device offers to remove with it. */
export interface Removal {
  device: { id: string; name: string };
  also: { id: string; name: string | null }[];
}
