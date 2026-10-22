<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '$lib/api';
  import { ago, short, your } from '$lib/model';
  import AddDevice from '$lib/components/AddDevice.svelte';
  import ApprovalCard from '$lib/components/ApprovalCard.svelte';
  import Apps from '$lib/components/Apps.svelte';
  import Devices from '$lib/components/Devices.svelte';
  import Events from '$lib/components/Events.svelte';
  import JoinForm from '$lib/components/JoinForm.svelte';
  import Passphrase from '$lib/components/Passphrase.svelte';
  import type { State } from '$lib/types';

  let bearer = $state<string | null>(null);
  let current = $state<State | null>(null);
  let problem = $state<'token' | 'unreachable' | null>(null);
  let now = $state(Date.now() / 1000);
  let answered = $state(new Set<string>());
  /** A ticket handed to this page to join with, as a link to it can carry. */
  let handed = $state<string | null>(null);

  const approvals = $derived((current?.approvals ?? []).filter((a) => !answered.has(a.id)));
  const mesh = $derived(current?.mesh ?? null);
  /** What the mesh's apps serve, each with the apps serving it: for a single one, only the one in use. */
  const servers = $derived(
    (mesh?.servers ?? []).map((s) => ({ ...s, apps: s.single ? s.apps.slice(0, 1) : s.apps })).filter((s) => s.apps.length),
  );

  /** Read the state, then wait for it to change, for as long as the page is open. */
  async function follow(alive: () => boolean) {
    while (alive()) {
      if (!bearer) {
        problem = 'token';
        await new Promise((done) => setTimeout(done, 1000));
        continue;
      }
      try {
        const read = await api.state(bearer);
        current = read;
        problem = null;
        const waiting = new Set(read.approvals.map((a) => a.id));
        answered = new Set([...answered].filter((id) => waiting.has(id)));
        await api.command(bearer, { op: 'wait', after: read.version, within_ms: 25_000 });
      } catch (e) {
        problem = e instanceof api.Unauthorized ? 'token' : 'unreachable';
        await new Promise((done) => setTimeout(done, 1000));
      }
    }
  }

  onMount(() => {
    let open = true;
    handed = api.ticketFromFragment();
    bearer = api.token();
    void follow(() => open);
    const adopt = () => {
      handed = api.ticketFromFragment() ?? handed;
      bearer = api.token() ?? bearer;
    };
    addEventListener('hashchange', adopt);
    const clock = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => {
      open = false;
      removeEventListener('hashchange', adopt);
      clearInterval(clock);
    };
  });
</script>

<svelte:head>
  <title>{approvals.length ? `(${approvals.length}) ` : ''}ErisOverlayNetwork</title>
</svelte:head>

<main>
  <header class="top">
    <p class="kicker">ErisOverlayNetwork</p>
    <h1>{current?.device.name ?? 'This device'}</h1>
    <span class="dot" class:live={current && !problem} title={problem ? 'Not connected' : 'Connected'}></span>
    {#if mesh}<p class="faint mesh" title={mesh.id}>mesh {short(mesh.id)}</p>{/if}
  </header>

  {#if problem === 'token'}
    <section class="note">
      <h2>Open this page from ErisOverlayNetwork</h2>
      <p>This link has no token, or one from before the daemon last started. Run <code>eon open</code>.</p>
    </section>
  {:else if problem === 'unreachable'}
    <section class="note">
      <h2>ErisOverlayNetwork is not answering</h2>
      <p>Start it with <code>eon serve</code>; this page reconnects by itself.</p>
    </section>
  {:else if current && bearer && !mesh}
    {#key handed}
      <JoinForm
        {bearer}
        joining={current.joining}
        offer={current.offer}
        hostname={current.device.hostname}
        {now}
        ticket={handed}
      />
    {/key}
  {:else if current && bearer && mesh && !mesh.member}
    <section class="note">
      <h2>This device was removed from its mesh</h2>
      <p>Another device removed it. A removed device never counts again; to come back, it joins anew with a new key.</p>
    </section>
  {:else if current && bearer && mesh}
    <section class="approvals">
      {#each approvals as prompt (prompt.id)}
        <ApprovalCard
          {prompt}
          {bearer}
          {now}
          onanswered={(id) => (answered = new Set([...answered, id]))}
        />
      {:else}
        <div class="note quiet">
          <h2>Nothing is waiting</h2>
          <p>When an app asks to be approved, or a new device asks to join, it shows up here.</p>
        </div>
      {/each}
    </section>

    <div class="adds">
      <AddDevice {bearer} {now} purpose="device" tickets={current.tickets} inviting={current.inviting} />
      <AddDevice {bearer} {now} purpose="app" tickets={current.tickets} />
    </div>

    <Devices {bearer} devices={mesh.devices} {now} />

    {#if servers.length}
      <section class="servers">
        <p class="kicker">What your apps serve</p>
        {#each servers as server (server.capability)}
          {#each server.apps as app (app.app)}
            <p>
              <span class="dot inline" class:live={app.reachable !== false}></span>
              <strong>{your(server.capability)}</strong>: {app.name}, approved on {app.device} {ago(app.approved_at, now)}
            </p>
          {/each}
        {/each}
      </section>
    {/if}

    <Apps {bearer} notes={current.notes} {now} />
    {#each servers.filter((s) => s.passphrase) as server (server.capability)}
      <Passphrase {bearer} capability={server.capability} server={server.apps[0].name} />
    {/each}
    <Events events={current.events} {now} />
  {/if}
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    gap: 22px;
    max-width: 600px;
    margin: 0 auto;
    padding: 28px 16px 40px;
  }

  .top {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 6px 12px;
    border-bottom: 1px solid var(--color-panel-3);
    padding-bottom: 14px;
  }

  .top .kicker,
  .top .mesh {
    grid-column: 1 / -1;
  }

  .top .mesh {
    margin: 0;
  }

  h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .dot {
    width: 8px;
    height: 8px;
    margin-bottom: 8px;
    border-radius: 50%;
    background: var(--failed);
  }

  .dot.inline {
    display: inline-block;
    margin: 0 6px 1px 0;
  }

  .dot.live {
    background: var(--working);
  }

  .approvals {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .adds {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 10px;
  }

  .adds > :global(section) {
    flex-basis: 100%;
  }

  .servers p:not(.kicker) {
    margin: 6px 0 0;
    color: var(--color-subtext);
    font-size: 12px;
    line-height: 1.5;
  }

  .note {
    border: 1px solid var(--color-panel-3);
    border-radius: 8px;
    background: var(--color-panel-1);
    padding: 18px;
  }

  .note.quiet {
    text-align: center;
  }

  .note h2 {
    margin: 0 0 6px;
    font-size: 14px;
    font-weight: 600;
  }

  .note p {
    margin: 4px 0 0;
    color: var(--color-subtext);
    font-size: 12px;
    line-height: 1.5;
  }

  code {
    color: var(--color-text);
    font-size: 11px;
  }
</style>
