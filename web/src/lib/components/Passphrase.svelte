<script lang="ts">
  import * as api from '$lib/api';
  import { newPassphraseProblem, refusal } from '$lib/model';

  let { bearer }: { bearer: string } = $props();

  let open = $state(false);
  let current = $state('');
  let next = $state('');
  let again = $state('');
  let busy = $state(false);
  let error = $state('');
  let done = $state('');

  const problem = $derived(newPassphraseProblem(next, again));

  async function save(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    done = '';
    try {
      const outcome = await api.command(bearer, { op: 'passphrase', current, new: next });
      if (outcome.ok) {
        done = 'The passphrase is changed.';
        current = next = again = '';
      } else {
        error = outcome.error === 'wrong_passphrase' ? 'That is not the current passphrase.' : refusal(outcome);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <section class="passphrase">
    <p class="kicker">ErisDB passphrase</p>
    <p class="how">
      Approving an app for ErisDB takes this passphrase, which ErisDB itself checks. Setting the first one, and letting
      a locked-out device guess again, happen where ErisDB runs.
    </p>
    <form onsubmit={save}>
      <input type="text" name="username" autocomplete="username" value="ErisDB" readonly hidden />
      <label>
        <span class="kicker">Current</span>
        <input class="field" type="password" bind:value={current} autocomplete="current-password" required disabled={busy} />
      </label>
      <label>
        <span class="kicker">New</span>
        <input class="field" type="password" bind:value={next} autocomplete="new-password" required disabled={busy} />
      </label>
      <label>
        <span class="kicker">Again</span>
        <input class="field" type="password" bind:value={again} autocomplete="new-password" required disabled={busy} />
      </label>
      {#if problem}<span class="faint">{problem}</span>{/if}
      <p class="error-line" role="alert">{error}</p>
      {#if done}<p class="done" role="status">{done}</p>{/if}
      <div class="actions">
        <button type="button" class="button" onclick={() => (open = false)}>Done</button>
        <button type="submit" class="button primary" disabled={busy || !!problem || !current || !next || next !== again}>
          Change
        </button>
      </div>
    </form>
  </section>
{:else}
  <div class="start">
    <button type="button" class="button" onclick={() => (open = true)}>Change the ErisDB passphrase</button>
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
