export interface Identity {
  kind: 'iroh' | 'browser';
  key: string;
}

/** A request waiting for the person. */
export interface Approval {
  id: string;
  service: string;
  client: string;
  requested: string[];
  identity: Identity;
  pid: number | null;
  executable: string | null;
  needs_passphrase: boolean;
  expires: number;
}

export interface State {
  device: { name: string | null; joined: boolean; grants: string[] };
  approvals: Approval[];
}

/** A decision's answer: `{ok: true}`, or why not. */
export interface Outcome {
  ok?: true;
  error?: string;
  message?: string;
  retry_after?: number;
}
