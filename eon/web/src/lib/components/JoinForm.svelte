<script lang="ts">
  import { untrack } from 'svelte';
  import * as api from '$lib/api';
  import { listed, refusal, remaining } from '$lib/model';
  import type { Offer, Outcome, State } from '$lib/types';

  // A device in no mesh yet joins one of two ways: it shows a QR code for a device already in, a
  // phone most likely, to scan; or the person types here the code another device shows.
  let {
    bearer,
    joining,
    offer,
    hostname,
    now,
    ticket: handed,
  }: {
    bearer: string;
    joining: State['joining'];
    offer: State['offer'];
    hostname: string | null;
    now: number;
    ticket: string | null;
  } = $props();

  let typed = $state(untrack(() => handed ?? ''));
  let name = $state(untrack(() => offer?.name ?? hostname ?? ''));
  let busy = $state(false);
  let error = $state('');
  /** The QR code could not be shown; showing it is left to the person. */
  let stalled = $state(false);
  /** The ticket as `show_join` answered, until the state the page reads has it. */
  let opened = $state<Offer | null>(null);

  const waiting = $derived(joining && !joining.error);
  const shown = $derived(offer ?? opened);
  const live = $derived(shown !== null && shown.expires > now);

  async function run(message: { op: string; [key: string]: unknown }): Promise<boolean> {
    busy = true;
    error = '';
    try {
      const outcome = await api.command(bearer, message);
      if (outcome.error) error = refusal(outcome);
      return !outcome.error;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return false;
    } finally {
      busy = false;
    }
  }

  async function show() {
    if (!name.trim()) return;
    busy = true;
    error = '';
    try {
      const answer = await api.command<Outcome & Partial<Offer>>(bearer, { op: 'show_join', name: name.trim() });
      if (answer.ticket) opened = answer as Offer;
      else {
        error = refusal(answer);
        stalled = true;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      stalled = true;
    } finally {
      busy = false;
    }
  }

  // The QR code is up whenever this device waits to be added, under the name in the field.
  $effect(() => {
    if (!live && !waiting && !busy && !stalled && name.trim()) void show();
  });

  function renamed() {
    if (shown && name.trim() && name.trim() !== shown.name) void show();
  }

  function join(event: SubmitEvent) {
    event.preventDefault();
    void run({ op: 'join', ticket: typed.trim(), name: name.trim() });
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
    <label>
      <span class="kicker">Name this device</span>
      <input class="field" bind:value={name} onchange={renamed} placeholder="laptop" required maxlength="64" disabled={busy} />
    </label>

    {#if offer && (offer.status === 'offered' || offer.status === 'accepted')}
      <div class="offer" role="alert">
        <h3>{offer.by} offers to add this device to your devices</h3>
        <p class="how">Your devices there: {listed(offer.devices ?? [])}</p>
        {#if offer.status === 'offered'}
          <p class="how">Join them only if these are your devices.</p>
          <div class="actions">
            <button type="button" class="button" disabled={busy} onclick={() => run({ op: 'decline_offer' })}>Not mine</button>
            <button type="button" class="button primary" disabled={busy} onclick={() => run({ op: 'accept_offer' })}>Join</button>
          </div>
        {:else}
          <p class="how">Joining; this page moves on by itself.</p>
        {/if}
      </div>
    {:else if shown && live}
      <div class="shown">
        <!-- The daemon drew this QR code from the ticket; it holds no script. -->
        <div class="qr">{@html shown.svg}</div>
        <p class="how">
          Scan this with your phone, or any device already among yours. Its person says yes there, and this page asks
          you to confirm. <span class="faint">{remaining(shown.expires, now)}</span>
        </p>
      </div>
    {:else if stalled}
      <div class="actions">
        <button type="button" class="button" disabled={busy || !name.trim()} onclick={() => ((stalled = false), show())}>
          Show a QR code
        </button>
      </div>
    {/if}

    {#if !offer || offer.status === 'showing'}
    <form onsubmit={join}>
      <label>
        <span class="kicker">Or type the code another device shows</span>
        <input class="field code-field" bind:value={typed} placeholder="K7QX-M2PA" autocomplete="off" spellcheck="false" disabled={busy} />
      </label>
      <p class="how">On one of your devices, choose <em>Add a device</em> or run <code>eon add</code>. A ticket pasted here works too.</p>
      <p class="error-line" role="alert">{error || joining?.error || ''}</p>
      <div class="actions">
        <button
          type="button"
          class="button"
          disabled={busy || !name.trim() || !!typed.trim()}
          onclick={() => run({ op: 'init', name: name.trim() })}
        >
          Start a new mesh
        </button>
        <button type="submit" class="button primary" disabled={busy || !typed.trim() || !name.trim()}>
          {busy ? 'Reaching it…' : 'Join'}
        </button>
      </div>
    </form>
    {/if}
  {/if}
</section>

<style>
  .join {
    display: flex;
    flex-direction: column;
    gap: 12px;
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

  h3 {
    margin: 0;
    font-size: 14px;
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

  .shown {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
  }

  .qr {
    width: 220px;
    max-width: 100%;
    border-radius: 8px;
    background: #fff;
    padding: 8px;
  }

  .qr :global(svg) {
    display: block;
    width: 100%;
    height: auto;
  }

  .offer {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border: 1px solid var(--color-orange);
    border-radius: 6px;
    background: var(--color-panel-1);
    padding: 12px;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-top: 1px solid var(--color-panel-3);
    padding-top: 12px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .code-field {
    font-family: var(--font-mono);
    letter-spacing: 0.06em;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
