<script lang="ts">
  import * as api from '$lib/api';
  import { refusal, remaining } from '$lib/model';
  import type { OpenTicket, Outcome, Shown } from '$lib/types';

  // A ticket for a new device, or for an app with no ErisAuth beside it, such as one in a browser.
  let {
    bearer,
    now,
    purpose,
    tickets,
  }: { bearer: string; now: number; purpose: 'device' | 'app'; tickets: OpenTicket[] } = $props();

  let shown = $state<Shown | null>(null);
  let busy = $state(false);
  let error = $state('');
  let copied = $state(false);

  // Once a newcomer redeems the ticket, its request takes over from this panel. The state the
  // page last read may be from before the ticket was opened, so it counts once it has shown it.
  let listed = $state(false);
  $effect(() => {
    const pending = shown !== null && tickets.some((t) => t.id === shown?.id && t.status === 'pending');
    if (pending) listed = true;
    else if (listed) {
      shown = null;
      listed = false;
    }
  });

  const label = $derived(purpose === 'device' ? 'Add a device' : 'Add a browser app');
  const live = $derived(shown !== null && shown.expires > now);
  const command = $derived(shown && purpose === 'device' ? `erisauth join --name NAME '${shown.ticket}'` : (shown?.ticket ?? ''));

  async function open() {
    busy = true;
    error = '';
    copied = false;
    try {
      const answer = await api.command<Outcome & Partial<Shown>>(bearer, {
        op: purpose === 'device' ? 'add' : 'add_app',
      });
      if (answer.ticket) {
        listed = false;
        shown = answer as Shown;
      }
      else error = refusal(answer);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function close() {
    if (shown) await api.command(bearer, { op: 'close', id: shown.id }).catch(() => null);
    shown = null;
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

{#if shown && live}
  <section class="add">
    <header>
      <div>
        <p class="kicker">{label}</p>
        <h2>{purpose === 'device' ? 'Join from the new device' : 'Give the app this ticket'}</h2>
      </div>
      <span class="faint" title="Time left to use this ticket">{remaining(shown.expires, now)}</span>
    </header>
    <!-- The daemon drew this QR code from the ticket; it holds no script. -->
    <div class="qr">{@html shown.svg}</div>
    <p class="how">
      {purpose === 'device'
        ? 'Scan it with the new device, or run this on it:'
        : 'Scan it with the app, or paste the ticket into it:'}
    </p>
    <div class="command">
      <code>{command}</code>
      <button type="button" class="button" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
    </div>
    <p class="how">Its request appears here with a code to compare; approve it once both screens match.</p>
    <div class="actions">
      <button type="button" class="button" onclick={close}>Close</button>
    </div>
  </section>
{:else}
  <div class="start">
    <p class="error-line" role="alert">{error}</p>
    <button type="button" class="button" disabled={busy} onclick={open}>{label}</button>
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
