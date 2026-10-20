<script lang="ts">
  import { untrack } from 'svelte';
  import * as api from '$lib/api';
  import { refusal } from '$lib/model';
  import type { State } from '$lib/types';

  let {
    bearer,
    joining,
    ticket: handed,
  }: { bearer: string; joining: State['joining']; ticket: string | null } = $props();

  let ticket = $state(untrack(() => handed ?? ''));
  let name = $state('');
  let busy = $state(false);
  let error = $state('');

  const waiting = $derived(joining && !joining.error);

  async function run(message: { op: string; [key: string]: unknown }) {
    busy = true;
    error = '';
    try {
      const outcome = await api.command(bearer, message);
      if (!outcome.ok) error = refusal(outcome);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function join(event: SubmitEvent) {
    event.preventDefault();
    void run({ op: 'join', ticket: ticket.trim(), name: name.trim() });
  }

  function start() {
    void run({ op: 'init', name: name.trim() });
  }
</script>

<section class="join">
  {#if waiting && joining}
    <p class="kicker">Joining</p>
    <h2>Compare this code with {joining.host}'s screen</h2>
    <strong class="code">{joining.fingerprint}</strong>
    <p class="how">Approve there only if both show the same code. This page moves on by itself.</p>
  {:else}
    <p class="kicker">Your devices</p>
    <h2>This device is in no mesh yet</h2>
    <p class="how">
      Join your other devices: on one of them, choose <em>Add a device</em> (or run <code>erisauth add</code>), and
      paste its ticket here. Or start a new mesh with this device as its first.
    </p>
    <form onsubmit={join}>
      <label>
        <span class="kicker">Name</span>
        <input class="field" bind:value={name} placeholder="laptop" required maxlength="64" disabled={busy} />
      </label>
      <label>
        <span class="kicker">Ticket</span>
        <textarea class="field" rows="3" bind:value={ticket} placeholder="eris://pair/…" disabled={busy}></textarea>
      </label>
      <p class="error-line" role="alert">{error || joining?.error || ''}</p>
      <div class="actions">
        <button type="button" class="button" disabled={busy || !name.trim() || !!ticket.trim()} onclick={start}>
          Start a new mesh
        </button>
        <button type="submit" class="button primary" disabled={busy || !ticket.trim() || !name.trim()}>
          {busy ? 'Reaching it…' : 'Join'}
        </button>
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
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
