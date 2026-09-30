<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '$lib/api';
  import AddDevice from '$lib/components/AddDevice.svelte';
  import ApprovalCard from '$lib/components/ApprovalCard.svelte';
  import JoinForm from '$lib/components/JoinForm.svelte';
  import type { State } from '$lib/types';

  let bearer = $state<string | null>(null);
  let current = $state<State | null>(null);
  let problem = $state<'token' | 'unreachable' | null>(null);
  let now = $state(Date.now() / 1000);
  let answered = $state(new Set<string>());
  /** A ticket handed to this page to join with, as a phone's link to it does. */
  let handed = $state<string | null>(null);

  const approvals = $derived((current?.approvals ?? []).filter((a) => !answered.has(a.id)));

  async function refresh() {
    if (!bearer) {
      problem = 'token';
      return;
    }
    try {
      current = await api.state(bearer);
      problem = null;
      const waiting = new Set(current.approvals.map((a) => a.id));
      answered = new Set([...answered].filter((id) => waiting.has(id)));
    } catch (e) {
      problem = e instanceof api.Unauthorized ? 'token' : 'unreachable';
    }
  }

  onMount(() => {
    handed = api.ticketFromFragment();
    bearer = api.token();
    void refresh();
    const adopt = () => {
      handed = api.ticketFromFragment() ?? handed;
      bearer = api.token();
      void refresh();
    };
    addEventListener('hashchange', adopt);
    const poll = setInterval(refresh, 1000);
    const clock = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => {
      removeEventListener('hashchange', adopt);
      clearInterval(poll);
      clearInterval(clock);
    };
  });
</script>

<svelte:head>
  <title>{approvals.length ? `(${approvals.length}) ` : ''}ErisAuth</title>
</svelte:head>

<main>
  <header class="top">
    <p class="kicker">ErisAuth</p>
    {#if current?.device.joined}
      <h1>{current.device.name}</h1>
    {:else}
      <h1>This device</h1>
    {/if}
    <span class="dot" class:live={current && !problem} title={problem ? 'Not connected' : 'Connected'}></span>
  </header>

  {#if problem === 'token'}
    <section class="note">
      <h2>Open this page from ErisAuth</h2>
      <p>This link has no token, or one from before the daemon last started. Run <code>erisauth open</code>.</p>
    </section>
  {:else if problem === 'unreachable'}
    <section class="note">
      <h2>ErisAuth is not answering</h2>
      <p>Start it with <code>erisauth serve</code>; this page reconnects by itself.</p>
    </section>
  {:else if current && !current.device.joined && bearer}
    {#key handed}
      <JoinForm {bearer} joining={current.joining} ticket={handed} />
    {/key}
  {:else if current && !approvals.length}
    <section class="note quiet">
      <h2>Nothing is waiting</h2>
      <p>When an app on this device asks to sign in, or a new device asks to join, it shows up here.</p>
      <p class="faint">Approves by itself: {current.device.grants.filter((g) => !g.startsWith('meta:') && !g.startsWith('erisauth')).join(', ') || 'nothing; everything takes the passphrase'}</p>
    </section>
  {:else if current && bearer}
    <section class="approvals">
      {#each approvals as approval (approval.id)}
        <ApprovalCard
          {approval}
          device={current.device.grants}
          {bearer}
          {now}
          onanswered={(id) => (answered = new Set([...answered, id]))}
        />
      {/each}
    </section>
  {/if}

  {#if current?.device.joined && bearer && !problem}
    <AddDevice {bearer} {now} waiting={approvals.map((a) => a.id)} />
  {/if}
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    gap: 18px;
    max-width: 560px;
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

  .top .kicker {
    grid-column: 1 / -1;
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

  .dot.live {
    background: var(--working);
  }

  .approvals {
    display: flex;
    flex-direction: column;
    gap: 14px;
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

  .note .faint {
    margin-top: 12px;
  }

  code {
    color: var(--color-text);
    font-size: 11px;
  }
</style>
