<script lang="ts">
  // Your year on DJINN GO: how much you listened, your top songs and
  // artists, your busiest month and the first song you played.
  import { api } from '../../lib/api'
  import Art from '../../lib/Art.svelte'
  import { toast } from '../../lib/state.svelte'
  import type { GoYear } from '../../lib/types'

  let year = $state<number | undefined>(undefined)
  let review = $state<GoYear | null>(null)

  $effect(() => {
    api
      .goYear(year)
      .then((r) => (review = r))
      .catch((e) => toast(e instanceof Error ? e.message : 'Could not load your year', true))
  })

  const n = (x: number) => x.toLocaleString()
  const month = (m: number, style: 'long' | 'short' = 'long') =>
    new Date(2000, m - 1, 1).toLocaleDateString([], { month: style })
  const listened = (minutes: number) =>
    minutes >= 120 ? `${n(Math.round(minutes / 60))} hours` : `${n(minutes)} minutes`
</script>

{#if review}
  {@const r = review}
  <div class="head">
    <h2>Your {r.year}</h2>
    {#if r.years.length > 1}
      <select aria-label="Year" value={r.year} onchange={(e) => (year = Number(e.currentTarget.value))}>
        {#each r.years as y (y)}<option value={y}>{y}</option>{/each}
      </select>
    {/if}
  </div>
  {#if !r.plays}
    <p class="muted">Nothing played on DJINN GO in {r.year} yet.</p>
  {:else}
    <div class="tiles">
      <div><strong>{listened(r.minutes)}</strong><span class="small muted">listened</span></div>
      <div><strong>{n(r.plays)}</strong><span class="small muted">plays</span></div>
      <div><strong>{n(r.songs)}</strong><span class="small muted">different songs</span></div>
      {#if r.requests}<div><strong>{n(r.requests)}</strong><span class="small muted">asked for at the party</span></div>{/if}
    </div>

    {#if r.top_songs.length}
      <h3>Your top songs</h3>
      <ol>
        {#each r.top_songs as s (s.track_id)}
          <li>
            {#if s.album_id}<a href="#/album/{s.album_id}">{s.title}</a>{:else}{s.title}{/if}{#if s.artist}<span class="muted"
                >{` · ${s.artist}`}</span
              >{/if}
            <span class="small muted">{n(s.plays)}×</span>
          </li>
        {/each}
      </ol>
    {/if}
    {#if r.top_artists.length}
      <h3>Your top artists</h3>
      <ol>
        {#each r.top_artists as a (a.artist_id)}
          <li><a href="#/artist/{a.artist_id}">{a.name}</a> <span class="small muted">{n(a.plays)}×</span></li>
        {/each}
      </ol>
    {/if}

    {#if r.busiest_month}
      {@const max = Math.max(1, ...r.months)}
      <h3>Busiest in {month(r.busiest_month)}</h3>
      <figure class="months" aria-label="Plays each month">
        {#each r.months as plays, i (i)}
          <div class="slot" title="{month(i + 1)}: {n(plays)} plays">
            <div class="bar" style:height="{(plays / max) * 100}%"></div>
            <span class="tick small muted">{month(i + 1, 'short').charAt(0)}</span>
          </div>
        {/each}
      </figure>
    {/if}

    {#if r.first}
      <h3>Your first song of {r.year}</h3>
      <a class="first" href={r.first.album_id ? `#/album/${r.first.album_id}` : undefined}>
        <Art hash={r.first.art_hash} size={56} />
        <span>
          <span>{r.first.title}</span>
          {#if r.first.artist}<span class="small muted">{r.first.artist}</span>{/if}
        </span>
      </a>
    {/if}
  {/if}
{:else}
  <p class="small muted">Loading...</p>
{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  h2 {
    margin: 12px 0;
  }

  h3 {
    margin: 18px 0 6px;
    font-size: 1rem;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 10px;
  }

  .tiles div {
    display: flex;
    flex-direction: column;
    padding: 12px;
    border-radius: 12px;
    background: var(--surface-2);
  }

  .tiles strong {
    font-size: 1.3rem;
  }

  ol {
    margin: 0;
    padding-left: 22px;
  }

  li {
    padding: 4px 0;
    overflow-wrap: anywhere;
  }

  .months {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 110px;
    margin: 0;
    padding-bottom: 18px;
    border-bottom: 1px solid var(--line);
  }

  .slot {
    position: relative;
    display: flex;
    flex: 1;
    align-items: flex-end;
    height: 100%;
  }

  .bar {
    width: 100%;
    min-height: 1px;
    border-radius: 4px 4px 0 0;
    background: var(--accent);
  }

  .tick {
    position: absolute;
    bottom: -18px;
    left: 0;
    right: 0;
    text-align: center;
  }

  .first {
    display: flex;
    align-items: center;
    gap: 12px;
    color: inherit;
    text-decoration: none;
  }

  .first > span {
    display: flex;
    flex-direction: column;
  }
</style>
