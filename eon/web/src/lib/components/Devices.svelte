<script lang="ts">
  import * as api from '$lib/api';
  import { ago, refusal, toRemove } from '$lib/model';
  import type { Device, Removal } from '$lib/types';

  let { bearer, devices, now }: { bearer: string; devices: Device[]; now: number } = $props();

  /** The device being renamed, and its new name as typed. */
  let renaming = $state<{ id: string; name: string } | null>(null);
  /** The removal being decided: what it offers, and the devices the person unticked. */
  let removing = $state<{ removal: Removal; kept: Set<string> } | null>(null);
  let busy = $state(false);
  let error = $state('');

  async function run(message: { op: string; [key: string]: unknown }): Promise<Record<string, unknown> | null> {
    busy = true;
    error = '';
    try {
      const outcome = await api.command(bearer, message);
      if (outcome.error) {
        error = refusal(outcome);
        return null;
      }
      return outcome;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return null;
    } finally {
      busy = false;
    }
  }

  async function rename(event: SubmitEvent) {
    event.preventDefault();
    if (!renaming) return;
    if (await run({ op: 'rename', device: renaming.id, name: renaming.name.trim() })) renaming = null;
  }

  async function offer(device: Device) {
    renaming = null;
    const removal = await run({ op: 'removal', device: device.id });
    if (removal) removing = { removal: removal as unknown as Removal, kept: new Set() };
  }

  function keep(id: string) {
    if (!removing) return;
    const kept = new Set(removing.kept);
    if (kept.has(id)) kept.delete(id);
    else kept.add(id);
    removing = { ...removing, kept };
  }

  async function remove() {
    if (!removing) return;
    if (await run({ op: 'remove', devices: toRemove(removing.removal, removing.kept) })) removing = null;
  }
</script>

<section class="devices">
  <p class="kicker">Devices</p>
  <ul>
    {#each devices as device (device.id)}
      <li>
        <span
          class="dot"
          class:live={device.reachable === true}
          class:unknown={device.reachable === null}
          title={device.reachable === null ? 'Not asked yet' : device.reachable ? 'Reachable' : 'Not reachable'}
        ></span>
        {#if renaming?.id === device.id}
          <form class="rename" onsubmit={rename}>
            <input class="field" bind:value={renaming.name} maxlength="64" required disabled={busy} aria-label="New name" />
            <button type="submit" class="button primary" disabled={busy || !renaming.name.trim()}>Rename</button>
            <button type="button" class="button" onclick={() => (renaming = null)}>Cancel</button>
          </form>
        {:else}
          <div class="who">
            <strong>{device.name}</strong>{#if device.this}<span class="faint this">this device</span>{/if}
            <p>
              {device.added_by_name ? `added by ${device.added_by_name}` : 'started the mesh'}, {ago(device.added_at, now)}
            </p>
          </div>
          <div class="row-actions">
            <button type="button" class="button" disabled={busy} onclick={() => (renaming = { id: device.id, name: device.name })}>
              Rename
            </button>
            <button type="button" class="button" disabled={busy} onclick={() => offer(device)}>Remove</button>
          </div>
        {/if}
      </li>
    {/each}
  </ul>

  {#if removing}
    <div class="removal" role="dialog" aria-label="Remove {removing.removal.device.name}">
      <h3>Remove {removing.removal.device.name}, for good?</h3>
      {#if removing.removal.also.length}
        <p class="how">It added these devices. They go with it unless you untick them:</p>
        {#each removing.removal.also as also (also.id)}
          <label class="also">
            <input type="checkbox" checked={!removing.kept.has(also.id)} onchange={() => keep(also.id)} disabled={busy} />
            <span>{also.name ?? also.id}</span>
          </label>
        {/each}
      {/if}
      <p class="how">A removed device never counts again; to come back it joins anew. Its apps stop counting everywhere.</p>
      <div class="actions">
        <button type="button" class="button" onclick={() => (removing = null)}>Cancel</button>
        <button type="button" class="button danger" disabled={busy} onclick={remove}>
          Remove {toRemove(removing.removal, removing.kept).length === 1 ? 'it' : `${toRemove(removing.removal, removing.kept).length} devices`}
        </button>
      </div>
    </div>
  {/if}
  <p class="error-line" role="alert">{error}</p>
</section>

<style>
  .devices {
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

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--failed);
  }

  .dot.live {
    background: var(--working);
  }

  .dot.unknown {
    background: var(--color-panel-6);
  }

  .who {
    flex: 1;
    min-width: 0;
  }

  .who strong {
    font-size: 13px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .this {
    margin-left: 8px;
  }

  .who p,
  .how {
    margin: 2px 0 0;
    color: var(--color-subtext);
    font-size: 11px;
    line-height: 1.5;
  }

  .row-actions,
  .rename,
  .actions {
    display: flex;
    gap: 6px;
  }

  .rename {
    flex: 1;
  }

  .removal {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border: 1px solid color-mix(in oklch, var(--failed) 45%, var(--color-panel-4));
    border-radius: 8px;
    background: var(--color-panel-2);
    padding: 14px 16px;
  }

  h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }

  .also {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 12px;
  }

  .also input {
    accent-color: var(--failed);
  }

  .actions {
    justify-content: flex-end;
  }

  .button.danger {
    border-color: transparent;
    background: var(--failed);
    color: var(--color-panel-0);
    font-weight: 600;
  }
</style>
