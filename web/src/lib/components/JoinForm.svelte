<script lang="ts">
  import { untrack } from 'svelte';
  import * as api from '$lib/api';
  import { parseCeiling } from '$lib/model';
  import type { State } from '$lib/types';

  let {
    bearer,
    joining,
    ticket: handed,
  }: { bearer: string; joining: State['joining']; ticket: string | null } = $props();

  let ticket = $state(untrack(() => handed ?? ''));
  let name = $state('');
  let ceiling = $state('');
  let busy = $state(false);
  let error = $state('');

  const waiting = $derived(joining && !joining.error && joining.fingerprint);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    try {
      const outcome = await api.join(bearer, ticket.trim(), name.trim(), parseCeiling(ceiling));
      if (!outcome.ok) error = outcome.message ?? outcome.error ?? 'ErisAuth could not join.';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="join">
  {#if waiting}
    <p class="kicker">Joining ErisDB</p>
    <h2>Compare this code with the approving screen</h2>
    <strong class="code">{joining?.fingerprint}</strong>
    <p class="how">Approve there only if both show the same code. This page moves on by itself.</p>
  {:else}
    <p class="kicker">Join ErisDB</p>
    <h2>This device has not joined yet</h2>
    <p class="how">
      Show a ticket on a device that has joined (<em>Add a device</em> on its page), or cut one with
      <code>erisdb pair</code>, and paste it here.
    </p>
    <form onsubmit={submit}>
      <label>
        <span class="kicker">Ticket</span>
        <textarea class="field" rows="3" bind:value={ticket} placeholder="erisdb://pair/…" required disabled={busy}></textarea>
      </label>
      <label>
        <span class="kicker">Name</span>
        <input class="field" bind:value={name} placeholder="phone" required maxlength="64" disabled={busy} />
      </label>
      <label>
        <span class="kicker">Approves by itself</span>
        <input class="field" bind:value={ceiling} placeholder="* (everything)" disabled={busy} />
        <span class="faint">Grants this device may approve for apps without the passphrase, such as notes:*</span>
      </label>
      <p class="error-line" role="alert">{error || joining?.error || ''}</p>
      <div class="actions">
        <button type="submit" class="button primary" disabled={busy || !ticket.trim() || !name.trim()}>{busy ? 'Reaching ErisDB…' : 'Join'}</button>
      </div>
    </form>
  {/if}
</section>

<style>
  .join {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border: 1px solid var(--color-panel-4);
    border-radius: 8px;
    background: var(--color-panel-2);
    padding: 16px;
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .how {
    margin: 0;
    color: var(--color-subtext);
    font-size: 12px;
    line-height: 1.5;
  }

  .code {
    align-self: center;
    margin: 8px 0;
    font: 600 24px/1 var(--font-mono);
    letter-spacing: 0.08em;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 4px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  textarea {
    resize: vertical;
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
