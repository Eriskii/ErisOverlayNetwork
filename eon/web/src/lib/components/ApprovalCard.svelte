<script lang="ts">
  import { untrack } from 'svelte';
  import * as api from '$lib/api';
  import { chosenGrants, grantKey, headline, refusal, remaining, short, your } from '$lib/model';
  import type { Prompt } from '$lib/types';

  let {
    prompt,
    bearer,
    now,
    onanswered,
  }: {
    prompt: Prompt;
    bearer: string;
    now: number;
    onanswered: (id: string) => void;
  } = $props();

  // Each card is one request, keyed by its id, so what it asks never changes under it.
  const asked = untrack(() => Object.entries(prompt.uses).flatMap(([c, grants]) => grants.map((g) => grantKey(c, g))));
  let chosen = $state(new Set(asked));
  let passphrase = $state('');
  let busy = $state(false);
  let error = $state('');

  const said = $derived(headline(prompt));
  const granted = $derived(chosenGrants(prompt, chosen));
  const count = $derived(asked.filter((key) => chosen.has(key)).length);
  const ready = $derived(
    !busy &&
      !prompt.busy &&
      (Object.keys(granted).length > 0 || prompt.serves.length > 0) &&
      (!prompt.passphrase || passphrase.length > 0),
  );

  function toggle(key: string) {
    const next = new Set(chosen);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    chosen = next;
    error = '';
  }

  async function answer(approve: boolean) {
    busy = true;
    error = '';
    try {
      const outcome = approve
        ? await api.command(bearer, {
            op: 'decide',
            id: prompt.id,
            grants: granted,
            ...(prompt.passphrase ? { passphrase } : {}),
          })
        : await api.command(bearer, { op: 'deny', id: prompt.id });
      if (outcome.ok) {
        onanswered(prompt.id);
        return;
      }
      error = refusal(outcome);
      if (outcome.error === 'wrong_passphrase') passphrase = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<article class="card">
  <span class="strip" class:enclave={prompt.passphrase || prompt.serves.some((s) => s.single)}></span>
  <div class="body">
    <header>
      <div class="who">
        <h2>{said.title}</h2>
        <p>{said.detail}</p>
      </div>
      <span class="faint" title="Time left to answer">{remaining(prompt.expires, now)}</span>
    </header>

    {#if prompt.fingerprint}
      <div class="compare">
        <p class="kicker">Compare with the {prompt.kind === 'device' ? 'new device' : "app's screen"}</p>
        <strong>{prompt.fingerprint}</strong>
        <p>Approve only if both screens show this code.</p>
      </div>
    {/if}

    {#if prompt.kind === 'app'}
      <dl>
        {#if prompt.package}
          <dt>app</dt>
          <dd><code>{prompt.package}</code></dd>
          <dt>signed</dt>
          <dd><code title={prompt.certificate ?? ''}>{prompt.certificate ? short(prompt.certificate) : 'unknown'}</code></dd>
        {:else if prompt.via === 'agent'}
          <dt>program</dt>
          <dd>
            <code>{prompt.program ?? 'unknown'}</code>{#if prompt.pid}<span class="faint pid">pid {prompt.pid}</span>{/if}
          </dd>
        {/if}
        <dt>key</dt>
        <dd><code title={prompt.key}>{short(prompt.key)}</code></dd>
      </dl>

      {#each Object.entries(prompt.uses).filter(([, grants]) => grants.length) as [capability, grants] (capability)}
        <fieldset>
          <legend class="kicker">It wants, in {your(capability)}</legend>
          {#each grants as grant (grant)}
            <label class="grant">
              <input
                type="checkbox"
                checked={chosen.has(grantKey(capability, grant))}
                onchange={() => toggle(grantKey(capability, grant))}
                disabled={busy}
              />
              <code>{grant}</code>
            </label>
          {/each}
        </fieldset>
      {/each}
    {/if}

    <form
      onsubmit={(event) => {
        event.preventDefault();
        void answer(true);
      }}
    >
      {#if prompt.passphrase}
        <!-- Password managers file the passphrase under this name. -->
        <input type="text" name="username" autocomplete="username" value="eon" readonly hidden />
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
        <button type="button" class="button" disabled={busy || prompt.busy} onclick={() => answer(false)}>Deny</button>
        <button type="submit" class="button primary" disabled={!ready}>
          {busy || prompt.busy ? 'Approving…' : count === asked.length ? 'Approve' : `Approve ${count} of ${asked.length}`}
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

  .strip.enclave {
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
    font-size: 16px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .who p {
    margin: 3px 0 0;
    color: var(--color-subtext);
    font-size: 12px;
    line-height: 1.5;
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

  .grant:hover {
    background: var(--color-overlay-hover);
  }

  .grant input {
    accent-color: var(--color-accent);
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
