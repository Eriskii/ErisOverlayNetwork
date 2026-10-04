<script lang="ts">
  import { untrack } from 'svelte';
  import * as api from '$lib/api';
  import { covers, needsPassphrase, remaining } from '$lib/model';
  import type { Approval } from '$lib/types';

  let {
    approval,
    device,
    bearer,
    now,
    onanswered,
  }: {
    approval: Approval;
    device: string[];
    bearer: string;
    now: number;
    onanswered: (id: string) => void;
  } = $props();

  // Each card is one request, keyed by its id, so its grants never change under it.
  let chosen = $state(untrack(() => new Set(approval.requested)));
  const optional = $derived(approval.requested.filter((grant) => !approval.essential.includes(grant)));
  const picked = $derived(optional.filter((grant) => chosen.has(grant)).length);
  let passphrase = $state('');
  let busy = $state(false);
  let error = $state('');

  const granted = $derived(approval.requested.filter((grant) => chosen.has(grant)));
  const needs = $derived(needsPassphrase(device, granted));
  const beyond = (grant: string) => !device.some((held) => covers(held, grant));
  const service = $derived(approval.service === 'erisdb' ? 'ErisDB' : approval.service);
  const signer = $derived(
    approval.certificate ? `${approval.certificate.slice(0, 8)}…${approval.certificate.slice(-8)}` : 'unknown',
  );
  const key = $derived(
    approval.identity ? `${approval.identity.key.slice(0, 8)}…${approval.identity.key.slice(-6)}` : 'unknown',
  );

  function toggle(grant: string) {
    const next = new Set(chosen);
    if (next.has(grant)) next.delete(grant);
    else next.add(grant);
    chosen = next;
    error = '';
  }

  const explain: Record<string, (retry?: number) => string> = {
    needs_passphrase: () => 'This takes the passphrase.',
    wrong_passphrase: () => 'That is not the passphrase.',
    locked: (retry) => `Too many wrong passphrases from this device. Try again in ${remaining(retry ?? 0, 0)}.`,
  };

  async function answer(approve: boolean) {
    busy = true;
    error = '';
    try {
      const outcome = approve
        ? await api.approve(bearer, approval.id, granted, needs ? passphrase : undefined)
        : await api.deny(bearer, approval.id);
      if (outcome.ok) {
        onanswered(approval.id);
        return;
      }
      const code = outcome.error ?? '';
      error = explain[code]?.(outcome.retry_after) ?? outcome.message ?? code ?? 'ErisAuth refused.';
      if (code === 'wrong_passphrase') passphrase = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<article class="card">
  <span class="strip" class:beyond={needs}></span>
  <div class="body">
    <header>
      <div class="who">
        <h2>{approval.client}</h2>
        <p>{approval.kind === 'device' ? `asks to join ${service}` : `asks to use ${service}`}</p>
      </div>
      <span class="faint" title="Time left to answer">{remaining(approval.expires, now)}</span>
    </header>

    {#if approval.kind === 'device'}
      <div class="compare">
        <p class="kicker">Compare with the new device</p>
        <strong>{approval.fingerprint ?? '?'}</strong>
        <p>Approve only if both screens show this code.</p>
      </div>
    {:else}
      <dl>
        {#if approval.package}
          <dt>app</dt>
          <dd><code>{approval.package}</code></dd>
          <dt>signed</dt>
          <dd><code title={approval.certificate ?? ''}>{signer}</code></dd>
        {:else}
          <dt>program</dt>
          <dd><code>{approval.executable ?? 'unknown'}</code>{#if approval.pid}<span class="faint pid">pid {approval.pid}</span>{/if}</dd>
        {/if}
        <dt>key</dt>
        <dd><code title={approval.identity?.key}>{approval.identity?.kind} {key}</code></dd>
      </dl>
    {/if}

    <fieldset>
      <legend class="kicker">It wants</legend>
      {#if approval.essential.length}
        <label class="grant fixed" title={approval.essential.join('\n')}>
          <input type="checkbox" checked disabled />
          <span>Act as an ErisAuth device</span>
          <span class="faint">approve pairings, use the passphrase, pair with your other devices</span>
        </label>
      {/if}
      {#each optional as grant (grant)}
        <label class="grant">
          <input type="checkbox" checked={chosen.has(grant)} onchange={() => toggle(grant)} disabled={busy} />
          <code>{grant}</code>
          {#if beyond(grant)}<span class="tag">passphrase</span>{/if}
        </label>
      {/each}
    </fieldset>

    <form
      onsubmit={(event) => {
        event.preventDefault();
        void answer(true);
      }}
    >
      {#if needs && granted.length}
        <!-- Password managers file the passphrase under this name. -->
        <input type="text" name="username" autocomplete="username" value="ErisAuth" readonly hidden />
        <input
          class="field"
          type="password"
          name="passphrase"
          autocomplete="current-password"
          placeholder="Passphrase"
          aria-label="Passphrase"
          bind:value={passphrase}
          disabled={busy}
        />
      {/if}
      <p class="error-line" role="alert">{error}</p>
      <div class="actions">
        <button type="button" class="button" disabled={busy} onclick={() => answer(false)}>Deny</button>
        <button type="submit" class="button primary" disabled={busy || !granted.length || (needs && !passphrase)}>
          {picked === optional.length ? 'Approve' : `Approve ${picked} of ${optional.length}`}
        </button>
      </div>
    </form>
  </div>
</article>

<style>
  .card {
    display: grid;
    grid-template-columns: 4px minmax(0, 1fr);
    border: 1px solid var(--color-panel-4);
    border-radius: 8px;
    background: var(--color-panel-2);
    overflow: hidden;
  }

  .strip {
    background: var(--color-accent);
  }

  .strip.beyond {
    background: var(--color-orange);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px 16px 14px 14px;
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .who p {
    margin: 3px 0 0;
    color: var(--color-subtext);
    font-size: 12px;
  }

  dl {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 4px 12px;
    margin: 0;
    font-size: 11px;
  }

  dt {
    color: var(--color-subsubtext);
    font: 600 9px/1.6 var(--font-mono);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--color-subtext);
  }

  fieldset {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    border: 0;
    padding: 0;
  }

  legend {
    margin-bottom: 6px;
    padding: 0;
  }

  .compare {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--color-panel-4);
    border-radius: 6px;
    background: var(--color-panel-1);
    padding: 12px;
  }

  .compare strong {
    color: var(--color-text);
    font: 600 22px/1 var(--font-mono);
    letter-spacing: 0.08em;
  }

  .compare p:last-child {
    margin: 0;
    color: var(--color-subtext);
    font-size: 11px;
  }

  .pid {
    margin-left: 8px;
    white-space: nowrap;
  }

  .grant {
    display: flex;
    align-items: center;
    gap: 9px;
    border-radius: 5px;
    padding: 5px 6px;
    font-size: 12px;
    cursor: pointer;
  }

  .grant.fixed {
    cursor: default;
  }

  .grant.fixed .faint {
    margin-left: auto;
  }

  .grant:hover {
    background: var(--color-overlay-hover);
  }

  .grant input {
    accent-color: var(--color-accent);
  }

  .tag {
    margin-left: auto;
    border: 1px solid color-mix(in oklch, var(--color-orange) 55%, transparent);
    border-radius: 999px;
    padding: 1px 7px;
    color: var(--color-orange);
    font: 9px/1.4 var(--font-mono);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
