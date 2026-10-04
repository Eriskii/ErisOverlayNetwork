<script lang="ts">
  import * as api from '$lib/api';
  import { passphraseProblem, type PassphraseAction } from '$lib/model';

  let { bearer }: { bearer: string } = $props();

  let open = $state(false);
  let current = $state('');
  let next = $state('');
  let again = $state('');
  let busy = $state(false);
  let error = $state('');
  let done = $state('');

  const mismatch = $derived(again.length > 0 && again !== next);
  const short = $derived(next.length > 0 && next.length < 8);

  async function run(action: PassphraseAction, request: () => Promise<{ ok?: true }>, success: string) {
    busy = true;
    error = '';
    done = '';
    try {
      const outcome = await request();
      error = passphraseProblem(outcome, action);
      if (!error) {
        done = success;
        current = next = again = '';
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function save(event: SubmitEvent) {
    event.preventDefault();
    const action = current ? 'change' : 'reset';
    void run(action, () => api.passphrase(bearer, current, next), 'The passphrase is set.');
  }

  function unlock() {
    void run('unlock', () => api.unlock(bearer), 'Every device may guess again.');
  }
</script>

{#if open}
  <section class="passphrase">
    <header>
      <div>
        <p class="kicker">Passphrase</p>
        <h2>Approvals beyond what a device may do itself</h2>
      </div>
    </header>
    <p class="how">
      Type the current passphrase to change it. Leave it empty to set one without it: that is how the first
      passphrase is set, and only a device allowed to reset it may.
    </p>
    <form onsubmit={save}>
      <label>
        <span class="kicker">Current passphrase</span>
        <input class="field" type="password" bind:value={current} autocomplete="current-password" disabled={busy} />
      </label>
      <label>
        <span class="kicker">New passphrase</span>
        <input class="field" type="password" bind:value={next} autocomplete="new-password" required disabled={busy} />
        {#if short}<span class="faint">At least 8 characters.</span>{/if}
      </label>
      <label>
        <span class="kicker">Again</span>
        <input class="field" type="password" bind:value={again} autocomplete="new-password" required disabled={busy} />
        {#if mismatch}<span class="faint">The two differ.</span>{/if}
      </label>
      <p class="error-line" role="alert">{error}</p>
      {#if done}<p class="done" role="status">{done}</p>{/if}
      <div class="actions">
        <button type="button" class="button" disabled={busy} onclick={unlock}>Let every device guess again</button>
        <button type="submit" class="button primary" disabled={busy || short || mismatch || !next || next !== again}>
          Save
        </button>
      </div>
    </form>
    <div class="actions">
      <button type="button" class="button" onclick={() => (open = false)}>Done</button>
    </div>
  </section>
{:else}
  <div class="start">
    <button type="button" class="button" onclick={() => (open = true)}>Passphrase</button>
  </div>
{/if}

<style>
  .passphrase {
    display: flex;
    flex-direction: column;
    gap: 12px;
    border: 1px solid var(--color-panel-4);
    border-radius: 8px;
    background: var(--color-panel-2);
    padding: 14px 16px;
  }

  h2 {
    margin: 4px 0 0;
    font-size: 15px;
    font-weight: 600;
  }

  .how {
    margin: 0;
    color: var(--color-subtext);
    font-size: 12px;
    line-height: 1.5;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .done {
    margin: 0;
    color: var(--working);
    font-size: 12px;
  }

  .actions,
  .start {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
