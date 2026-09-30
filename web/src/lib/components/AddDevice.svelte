<script lang="ts">
  import * as api from '$lib/api';
  import { remaining } from '$lib/model';
  import type { Added } from '$lib/types';

  let { bearer, now, waiting }: { bearer: string; now: number; waiting: string[] } = $props();

  let added = $state<Added | null>(null);
  let busy = $state(false);
  let error = $state('');
  let copied = $state(false);

  // Once the new device redeems the ticket, its request takes over from this panel.
  $effect(() => {
    if (added?.id && waiting.includes(added.id)) added = null;
  });

  const command = $derived(added?.ticket ? `erisauth join '${added.ticket}'` : '');
  const live = $derived(added?.expires !== undefined && added.expires > now);

  async function open() {
    busy = true;
    error = '';
    copied = false;
    try {
      const answer = await api.add(bearer);
      if (answer.ticket) added = answer;
      else error = answer.message ?? answer.error ?? 'ErisAuth could not cut a ticket.';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      copied = true;
    } catch {
      copied = false;
    }
  }
</script>

{#if added && live}
  <section class="add">
    <header>
      <div>
        <p class="kicker">Add a device</p>
        <h2>Join from the new device</h2>
      </div>
      <span class="faint" title="Time left to use this ticket">{remaining(added.expires ?? 0, now)}</span>
    </header>
    <!-- The daemon drew this QR code from the ticket; it holds no script. -->
    <div class="qr">{@html added.svg}</div>
    <p class="how">On the new device, run:</p>
    <div class="command">
      <code>{command}</code>
      <button type="button" class="button" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
    </div>
    <p class="how">Its request appears here with a code to compare; approve it once both screens match.</p>
    <div class="actions">
      <button type="button" class="button" onclick={() => (added = null)}>Done</button>
    </div>
  </section>
{:else}
  <div class="start">
    <button type="button" class="button" disabled={busy} onclick={open}>Add a device</button>
    <p class="error-line" role="alert">{error}</p>
  </div>
{/if}

<style>
  .add {
    display: flex;
    flex-direction: column;
    gap: 12px;
    border: 1px solid var(--color-panel-4);
    border-radius: 8px;
    background: var(--color-panel-2);
    padding: 14px 16px;
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
  }

  h2 {
    margin: 4px 0 0;
    font-size: 15px;
    font-weight: 600;
  }

  .qr {
    align-self: center;
    width: 220px;
    border-radius: 8px;
    background: #fff;
    padding: 8px;
  }

  .qr :global(svg) {
    display: block;
    width: 100%;
    height: auto;
  }

  .how {
    margin: 0;
    color: var(--color-subtext);
    font-size: 12px;
  }

  .command {
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--color-panel-4);
    border-radius: 6px;
    background: var(--color-panel-0);
    padding: 6px 6px 6px 10px;
  }

  .command code {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: var(--color-subtext);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .actions,
  .start {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
