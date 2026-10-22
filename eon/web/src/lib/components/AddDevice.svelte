<script lang="ts">
  import * as api from '$lib/api';
  import { refusal, remaining } from '$lib/model';
  import type { Inviting, OpenTicket, Outcome, Shown } from '$lib/types';

  // A ticket for a new device, or for an app with no eon beside it, such as one in a browser. A
  // new device that shows a ticket of its own is added by pasting that ticket here instead.
  let {
    bearer,
    now,
    purpose,
    tickets,
    inviting = null,
  }: { bearer: string; now: number; purpose: 'device' | 'app'; tickets: OpenTicket[]; inviting?: Inviting | null } =
    $props();

  let shown = $state<Shown | null>(null);
  let busy = $state(false);
  let error = $state('');
  let copied = $state(false);
  /** A ticket the new device shows, pasted here. */
  let theirs = $state('');
  /** The new device the person is about to add by its ticket. */
  let adding = $state<string | null>(null);

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
  const command = $derived(
    shown && purpose === 'device' ? `eon join --name NAME ${shown.code ?? `'${shown.ticket}'`}` : (shown?.ticket ?? ''),
  );

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

  /** The name in a ticket a new device shows, to ask "add it?" by; null when it is not one. */
  function named(ticket: string): string | null {
    const payload = ticket.trim().match(/^eris:\/\/join\/([A-Za-z0-9_-]+)$/)?.[1];
    if (!payload) return null;
    try {
      const json = JSON.parse(atob(payload.replace(/-/g, '+').replace(/_/g, '/')));
      return typeof json.name === 'string' ? json.name : null;
    } catch {
      return null;
    }
  }

  async function invite() {
    busy = true;
    error = '';
    try {
      const outcome = await api.command(bearer, { op: 'invite', ticket: theirs.trim() });
      if (outcome.ok) {
        theirs = '';
        adding = null;
      } else error = refusal(outcome);
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
    {#if shown.code}
      <p class="how center">Or type this code on the new device:</p>
      <strong class="code">{shown.code}</strong>
    {/if}
    <p class="how">
      {purpose === 'device'
        ? 'At a terminal on the new device, run:'
        : 'Scan it with the app, or paste the ticket into it:'}
    </p>
    <div class="command">
      <code>{command}</code>
      <button type="button" class="button" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
    </div>
    <p class="how">Its request appears here with a code to compare; approve it once both screens match.</p>
    {#if purpose === 'device'}
      {#if adding}
        <div class="confirm">
          <p>Add <strong>{adding}</strong> to your devices?</p>
          <button type="button" class="button" onclick={() => (adding = null)}>Cancel</button>
          <button type="button" class="button primary" disabled={busy} onclick={invite}>Add</button>
        </div>
      {:else}
        <p class="how">If the new device shows a QR code of its own, scan it with your phone, or paste its ticket:</p>
        <form
          class="theirs"
          onsubmit={(event) => {
            event.preventDefault();
            adding = named(theirs);
            if (!adding) error = 'That is not the ticket a new device shows; it starts eris://join/.';
          }}
        >
          <input class="field" bind:value={theirs} placeholder="eris://join/…" disabled={busy} />
          <button type="submit" class="button" disabled={busy || !theirs.trim()}>Add it</button>
        </form>
      {/if}
      <p class="error-line" role="alert">{error}</p>
    {/if}
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

{#if purpose === 'device' && inviting && (inviting.status === 'waiting' || shown !== null)}
    <p class="how status" role="status">
      {inviting.status === 'waiting'
        ? `Adding ${inviting.name}: say yes on its screen.`
        : inviting.status === 'added'
          ? `${inviting.name} is added.`
          : inviting.status === 'declined'
            ? `${inviting.name} said no.`
            : `${inviting.name} was not added: ${inviting.error ?? 'something went wrong'}.`}
    </p>
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

  .center {
    text-align: center;
  }

  .code {
    align-self: center;
    font: 600 24px/1 var(--font-mono);
    letter-spacing: 0.1em;
  }

  .theirs,
  .confirm {
    display: flex;
    flex-basis: 100%;
    align-items: center;
    gap: 8px;
  }

  .theirs input {
    min-width: 0;
    flex: 1;
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .confirm p {
    flex: 1;
    margin: 0;
    font-size: 12px;
  }

  .status {
    flex-basis: 100%;
    text-align: right;
  }

  .actions,
  .start {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
