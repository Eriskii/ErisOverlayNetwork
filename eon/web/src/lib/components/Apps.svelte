<script lang="ts">
  import * as api from '$lib/api';
  import { ago, listed, refusal } from '$lib/model';
  import type { Note } from '$lib/types';

  let { bearer, notes, now }: { bearer: string; notes: Note[]; now: number } = $props();

  /** The note the person is about to revoke. */
  let revoking = $state<string | null>(null);
  let busy = $state(false);
  let error = $state('');

  async function revoke(note: Note) {
    busy = true;
    error = '';
    try {
      const outcome = await api.command(bearer, { op: 'revoke', note: note.id });
      if (outcome.error) error = refusal(outcome);
      else revoking = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  const standing = (note: Note) => (note.revoked ? 'revoked' : note.counts ? '' : 'no longer counts');

  /** What a note approves, in a line: what it uses, and what it serves. */
  function approves(note: Note): string {
    const uses = Object.keys(note.uses);
    const serves = note.serves.map((s) => s.capability);
    return [uses.length ? `uses ${listed(uses)}` : '', serves.length ? `serves ${listed(serves)}` : '']
      .filter(Boolean)
      .join(', ');
  }
</script>

<section class="apps">
  <p class="kicker">Apps this device approved</p>
  {#if notes.length}
    <ul>
      {#each notes as note (note.id)}
        <li class:gone={note.revoked || !note.counts}>
          <div class="what">
            <strong>{note.name}</strong>
            <span class="faint">{approves(note)}</span>
            {#if standing(note)}<span class="tag">{standing(note)}</span>{/if}
            <p>
              {Object.values(note.uses).flat().join(', ') || 'no grants'} · approved {ago(note.at, now)}
            </p>
            {#if note.program}<p><code>{note.program}</code></p>{/if}
          </div>
          {#if !note.revoked}
            {#if revoking === note.id}
              <div class="row-actions">
                <button type="button" class="button" onclick={() => (revoking = null)}>Keep</button>
                <button type="button" class="button danger" disabled={busy} onclick={() => revoke(note)}>Revoke everywhere</button>
              </div>
            {:else}
              <button type="button" class="button" onclick={() => (revoking = note.id)}>Revoke</button>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="how">None yet. Apps on this device ask for themselves; a browser app asks with a ticket from here.</p>
  {/if}
  <p class="error-line" role="alert">{error}</p>
</section>

<style>
  .apps {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  ul {
    display: flex;
    flex-direction: column;
    margin: 0;
    border: 1px solid var(--color-panel-3);
    border-radius: 8px;
    padding: 0;
    list-style: none;
    background: var(--color-panel-1);
  }

  li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
  }

  li + li {
    border-top: 1px solid var(--color-panel-3);
  }

  li.gone {
    opacity: 0.6;
  }

  .what {
    flex: 1;
    min-width: 0;
  }

  .what strong {
    margin-right: 6px;
    font-size: 13px;
    font-weight: 600;
  }

  .what p,
  .how {
    margin: 2px 0 0;
    color: var(--color-subtext);
    font-size: 11px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .tag {
    margin-left: 6px;
    border: 1px solid var(--color-panel-5);
    border-radius: 999px;
    padding: 1px 7px;
    color: var(--color-subtext);
    font: 9px/1.4 var(--font-mono);
  }

  .row-actions {
    display: flex;
    gap: 6px;
  }

  .button.danger {
    border-color: transparent;
    background: var(--failed);
    color: var(--color-panel-0);
    font-weight: 600;
  }
</style>
