<script lang="ts">
  // Admin > Stats: what's been played and asked for, at the party and on
  // DJINN GO, for a time range and optionally one person, plus clearing it.
  // "Last party" is the recap: from the first song after a long break.
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { GuestSummary, HistoryCounts, Stats, StatsFilter, User } from './types'

  type Range = 'party' | 'today' | 'week' | 'month' | 'year' | 'all'
  const ranges: { id: Range; label: string }[] = [
    { id: 'party', label: 'Last party' },
    { id: 'today', label: 'Today' },
    { id: 'week', label: '7 days' },
    { id: 'month', label: '30 days' },
    { id: 'year', label: 'This year' },
    { id: 'all', label: 'All time' },
  ]

  let range = $state<Range>('party')
  let person = $state<number | null>(null)
  let where = $state<'both' | 'party' | 'go'>('both')
  let stats = $state<Stats | null>(null)
  let noParty = $state(false)
  let loading = $state(false)
  let people = $state<{ id: number; name: string }[]>([])
  let clearing = $state<HistoryCounts | null>(null)

  $effect(() => {
    Promise.all([api.users().catch(() => [] as User[]), api.guests().catch(() => [] as GuestSummary[])]).then(
      ([users, guests]) => {
        people = [
          ...users.map((u) => ({ id: u.id, name: u.display_name || u.username })),
          ...guests.map((g) => ({ id: g.id, name: `${g.name} (guest)` })),
        ].sort((a, b) => a.name.localeCompare(b.name))
      },
    )
  })

  function since(r: Range): number | null {
    const now = new Date()
    switch (r) {
      case 'today':
        return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
      case 'week':
        return Date.now() - 7 * 86_400_000
      case 'month':
        return Date.now() - 30 * 86_400_000
      case 'year':
        return new Date(now.getFullYear(), 0, 1).getTime()
      default:
        return null
    }
  }

  /** The filter in effect; for the last party, from its start. */
  const filter = $derived<StatsFilter>({
    from: range === 'party' ? (stats?.from ?? null) : since(range),
    user: person,
    party: where !== 'go',
    go: where !== 'party',
  })

  async function load() {
    loading = true
    try {
      if (range === 'party') {
        // The recap says where the party starts; other filters reuse it.
        const recap = await api.recap()
        noParty = recap === null
        stats =
          recap && (person !== null || where === 'go')
            ? await api.stats({ from: recap.from, user: person, party: where !== 'go', go: where !== 'party' })
            : recap
      } else {
        noParty = false
        const from = since(range)
        stats = await api.stats({ from, user: person, party: where !== 'go', go: where !== 'party' })
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load stats', true)
    } finally {
      loading = false
    }
  }

  $effect(() => {
    void range
    void person
    void where
    void load()
  })

  async function askClear() {
    try {
      clearing = await api.historyCounts(filter)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not count that', true)
    }
  }

  async function clear() {
    try {
      const done = await api.clearHistory(filter)
      toast(`Cleared ${n(done.party_plays + done.go_plays)} plays and ${n(done.requests)} requests`)
      clearing = null
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not clear it', true)
    }
  }

  const n = (x: number) => x.toLocaleString()
  const count = (x: number, one: string, many: string) => `${n(x)} ${x === 1 ? one : many}`
  const hours = (ms: number) => {
    const h = ms / 3_600_000
    return h >= 10 ? `${n(Math.round(h))} hours` : h >= 1 ? `${h.toFixed(1)} hours` : `${Math.round(ms / 60_000)} min`
  }
  const hourLabel = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString([], { hour: 'numeric' })
  const dayNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
  const csv = $derived(
    `/api/stats/export.csv?${new URLSearchParams({
      ...(filter.from != null ? { from: String(filter.from) } : {}),
      ...(filter.user != null ? { user: String(filter.user) } : {}),
      party: String(filter.party),
      go: String(filter.go),
    })}`,
  )

  /** Every hour (or day) with its count, for the bars. */
  function filled(rows: [number, number][], size: number): number[] {
    const out = Array(size).fill(0)
    for (const [k, v] of rows) out[k] = v
    return out
  }
  const byHour = $derived(stats ? filled(stats.by_hour, 24) : [])
  const byDay = $derived(stats ? filled(stats.by_weekday, 7) : [])
  const peak = (values: number[]) => values.indexOf(Math.max(...values))
</script>

{#snippet bars(values: number[], label: (i: number) => string, title: string)}
  {@const max = Math.max(1, ...values)}
  <figure class="bars" aria-label={title}>
    <div class="plot">
      {#each values as value, i (i)}
        <div class="slot" title="{label(i)}: {n(value)} plays">
          <div class="bar" style:height="{(value / max) * 100}%"></div>
        </div>
      {/each}
    </div>
    <div class="ticks small muted" style:grid-template-columns="repeat({values.length}, 1fr)">
      {#each values as _, i (i)}
        <span>{values.length > 12 ? (i % 6 === 0 ? label(i) : '') : label(i)}</span>
      {/each}
    </div>
    <table class="sr-only">
      <caption>{title}</caption>
      <tbody>
        {#each values as value, i (i)}<tr><th>{label(i)}</th><td>{value}</td></tr>{/each}
      </tbody>
    </table>
  </figure>
{/snippet}

{#snippet songList(title: string, songs: Stats['top_songs'], skips = false)}
  {#if songs.length}
    <h3>{title}</h3>
    <ol>
      {#each songs.slice(0, 10) as s (s.track_id)}
        <li>
          {#if s.album_id}<a href="#/album/{s.album_id}">{s.title}</a>{:else}{s.title}{/if}
          {#if s.artist}<span class="muted">{` · ${s.artist}`}</span>{/if}
          <span class="count small muted">
            {skips ? `${n(s.skips)} of ${n(s.plays)} skipped` : count(s.plays, 'play', 'plays')}
          </span>
        </li>
      {/each}
    </ol>
  {/if}
{/snippet}

<section class="card">
  <div class="filters">
    <div class="chips" role="group" aria-label="Time">
      {#each ranges as r (r.id)}
        <button class:active={range === r.id} aria-pressed={range === r.id} onclick={() => (range = r.id)}>{r.label}</button>
      {/each}
    </div>
    <div class="actions">
      <select bind:value={person} aria-label="Whose">
        <option value={null}>Everyone</option>
        {#each people as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
      <select bind:value={where} aria-label="Where">
        <option value="both">Party and GO</option>
        <option value="party">Party only</option>
        <option value="go">GO only</option>
      </select>
    </div>
  </div>

  {#if noParty}
    <p class="muted">No party yet: nothing has played.</p>
  {:else if stats}
    {#if range === 'party' && stats.from}
      <p class="small muted">
        Since {new Date(stats.from).toLocaleString([], { weekday: 'long', hour: 'numeric', minute: '2-digit' })}, the first song after a
        break of six hours or more.
      </p>
    {/if}
    <div class="tiles">
      <div><strong>{n(stats.summary.plays)}</strong><span class="small muted">plays</span></div>
      <div><strong>{hours(stats.summary.listened_ms)}</strong><span class="small muted">listened</span></div>
      <div><strong>{n(stats.summary.songs)}</strong><span class="small muted">different songs</span></div>
      <div><strong>{n(stats.summary.requests)}</strong><span class="small muted">requests</span></div>
      <div><strong>{n(stats.summary.requesters)}</strong><span class="small muted">people asking</span></div>
      <div><strong>{n(stats.summary.skips)}</strong><span class="small muted">skipped</span></div>
    </div>
  {:else if loading}
    <p class="small muted">Loading...</p>
  {/if}
</section>

{#if stats && stats.summary.plays + stats.summary.requests > 0}
  <section class="card">
    <h2>Top</h2>
    {@render songList('Songs played most', stats.top_songs)}
    {#if stats.top_requested.length}
      <h3>Asked for most</h3>
      <ol>
        {#each stats.top_requested.slice(0, 10) as r (r.track_id)}
          <li>
            {r.title ?? '(removed song)'}{#if r.artist}<span class="muted">{` · ${r.artist}`}</span>{/if}
            <span class="count small muted">{count(r.requests, 'request', 'requests')}</span>
          </li>
        {/each}
      </ol>
    {/if}
    {#if stats.top_artists.length}
      <h3>Artists</h3>
      <ol>
        {#each stats.top_artists.slice(0, 10) as a (a.artist_id)}
          <li><a href="#/artist/{a.artist_id}">{a.name}</a> <span class="count small muted">{count(a.plays, 'play', 'plays')}</span></li>
        {/each}
      </ol>
    {/if}
    {#if stats.top_requesters.length}
      <h3>Who asked most</h3>
      <ol>
        {#each stats.top_requesters.slice(0, 10) as p, i (i)}
          <li>{p.name ?? 'Someone'} <span class="count small muted">{count(p.requests, 'request', 'requests')}</span></li>
        {/each}
      </ol>
    {/if}
    {@render songList('Skipped most', stats.most_skipped, true)}
  </section>

  {#if stats.summary.plays}
    <section class="card">
      <h2>When</h2>
      <p class="small muted">
        Busiest around {hourLabel(peak(byHour))}{#if range !== 'party' && range !== 'today'}, and on {dayNames[peak(byDay)]}s{/if}. This
        computer's time.
      </p>
      {@render bars(byHour, hourLabel, 'Plays by hour of the day')}
      {#if range !== 'party' && range !== 'today'}
        {@render bars(byDay, (i) => dayNames[i], 'Plays by day of the week')}
      {/if}
    </section>
  {/if}
{/if}

<section class="card">
  <h2>Clear this history</h2>
  <p class="small muted">
    Forgets the plays and requests in the view above (its time, person and where). What Smart play learned from them goes
    too, and top songs on artist pages count again from what's left. Save a backup first (Admin > System) if you might want
    them back.
  </p>
  <div class="actions">
    <a class="button small-button" href={csv} download>Download the lists as a spreadsheet</a>
    <button class="small-button" onclick={askClear}>Clear this history</button>
  </div>
  {#if clearing}
    {@const total = clearing.party_plays + clearing.go_plays + clearing.requests}
    {#if total === 0}
      <p class="small">Nothing to clear.</p>
    {:else}
      <p class="small">
        This removes {n(clearing.party_plays)} party plays, {n(clearing.go_plays)} GO plays and {n(clearing.requests)} requests.
      </p>
      <div class="actions">
        <button class="danger small-button" onclick={clear}>Clear them</button>
        <button class="small-button" onclick={() => (clearing = null)}>Cancel</button>
      </div>
    {/if}
  {/if}
</section>

<style>
  .filters {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .chips,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chips button {
    min-height: 36px;
    padding: 0 12px;
    border-radius: 999px;
    font-size: 0.875rem;
  }

  .chips button.active {
    background: var(--accent);
    color: var(--accent-text);
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
    gap: 10px;
  }

  .tiles div {
    display: flex;
    flex-direction: column;
    padding: 10px 12px;
    border-radius: 10px;
    background: var(--surface-2);
  }

  .tiles strong {
    font-size: 1.4rem;
    font-variant-numeric: tabular-nums;
  }

  h3 {
    margin: 10px 0 4px;
    font-size: 0.95rem;
  }

  ol {
    margin: 0;
    padding-left: 22px;
  }

  li {
    padding: 3px 0;
    overflow-wrap: anywhere;
  }

  .count {
    margin-left: 6px;
    white-space: nowrap;
  }

  .bars {
    margin: 8px 0 14px;
  }

  .plot {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 110px;
    border-bottom: 1px solid var(--line);
  }

  .slot {
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

  .slot:hover .bar {
    opacity: 0.75;
  }

  .ticks {
    display: grid;
    margin-top: 4px;
    font-size: 0.7rem;
    white-space: nowrap;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
