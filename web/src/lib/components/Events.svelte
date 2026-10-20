<script lang="ts">
  import { ago } from '$lib/model';
  import type { Event } from '$lib/types';

  let { events, now }: { events: Event[]; now: number } = $props();

  const latest = $derived([...events].reverse().slice(0, 8));
</script>

{#if latest.length}
  <section class="events">
    <p class="kicker">Lately</p>
    <ul>
      {#each latest as event (event.seq)}
        <li><span>{event.text}</span><span class="faint">{ago(event.at, now)}</span></li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .events {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    color: var(--color-subtext);
    font-size: 12px;
  }
</style>
