<script lang="ts">
  // Admin > Home: how DJINN is doing at a glance. Each line says whether
  // it's fine, worth a look, or wrong, and links to where to fix it.
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { toast } from './state.svelte'
  import type {
    About,
    LyricsImportReport,
    ProtectedStatus,
    ProblemSummary,
    ScheduledCheck,
    Stats,
    SystemStatus,
  } from './types'

  type Level = 'ok' | 'warn' | 'bad'
  type Line = { level: Level; label: string; detail: string; href?: string }

  let status = $state<SystemStatus | null>(null)
  let problems = $state<ProblemSummary | null>(null)
  let nightly = $state<ScheduledCheck | null>(null)
  let about = $state<About | null>(null)
  let lyricsImport = $state<LyricsImportReport | null>(null)
  let lock = $state<ProtectedStatus | null>(null)
  // Everything ever played and asked for, party and GO (loaded once).
  let allTime = $state<Stats | null>(null)

  async function load() {
    try {
      status = await api.systemStatus()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the status', true)
    }
  }

  $effect(() => {
    void load()
    api.libraryProblems(true).then((p) => (problems = p)).catch(() => {})
    api.scheduledCheck().then((c) => (nightly = c)).catch(() => {})
    api.about().then((a) => (about = a)).catch(() => {})
    api.protectedStatus().then((p) => (lock = p)).catch(() => {})
    api.lyricsImport().then((i) => (lyricsImport = i.report)).catch(() => {})
    api
      .stats({ party: true, go: true })
      .then((s) => (allTime = s))
      .catch(() => {})
    const timer = setInterval(() => void load(), 10_000)
    return () => clearInterval(timer)
  })

  const n = (x: number) => x.toLocaleString()
  const size = (bytes: number) =>
    bytes >= 1e12
      ? `${(bytes / 1e12).toFixed(1)} TB`
      : bytes >= 1e9
        ? `${(bytes / 1e9).toFixed(1)} GB`
        : `${Math.round(bytes / 1e6)} MB`
  /** A time in whole hours, or minutes when under one. */
  const hoursOf = (ms: number) =>
    ms >= 3_600_000 ? `${n(Math.floor(ms / 3_600_000))} h` : `${Math.floor(ms / 60_000)} min`
  /** The same as days and hours, for two days or more. */
  const days = (ms: number) => {
    const hours = Math.floor(ms / 3_600_000)
    return hours >= 48 ? ` (${n(Math.floor(hours / 24))} days ${hours % 24} h)` : ''
  }
  const weekdays = ['Sundays', 'Mondays', 'Tuesdays', 'Wednesdays', 'Thursdays', 'Fridays', 'Saturdays']
  const hourName = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString([], { hour: 'numeric' })
  const busiest = (rows: [number, number][]) =>
    rows.reduce<[number, number] | null>((best, row) => (!best || row[1] > best[1] ? row : best), null)

  const listening = $derived.by(() => {
    if (!allTime || !status) return null
    const sum = allTime.summary
    const hour = busiest(allTime.by_hour)
    const weekday = busiest(allTime.by_weekday)
    const first = allTime.by_day[0]?.[0] ?? null
    return {
      sum,
      share: status.songs ? Math.round((sum.songs / status.songs) * 100) : 0,
      song: allTime.top_songs[0] ?? null,
      artist: allTime.top_artists[0] ?? null,
      requested: allTime.top_requested[0] ?? null,
      hour: hour && hour[1] ? hourName(hour[0]) : null,
      weekday: weekday && weekday[1] ? weekdays[weekday[0]] : null,
      since: first ? new Date(`${first}T12:00:00`).toLocaleDateString([], { year: 'numeric', month: 'long', day: 'numeric' }) : null,
    }
  })

  const ago = (ms: number) => {
    const days = Math.floor((Date.now() - ms) / 86_400_000)
    return days < 1 ? 'today' : days === 1 ? 'yesterday' : `${days} days ago`
  }

  const music = $derived.by((): Line[] => {
    if (!status) return []
    const lines: Line[] = []
    const state = { idle: 'Nothing playing', playing: 'Playing', paused: 'Paused' }[status.state]
    const playing = status.outputs.outputs.filter((o) => o.state === 'playing')
    lines.push({
      level: status.outputs.error ? 'bad' : playing.length ? 'ok' : 'warn',
      label: state,
      detail: status.outputs.error ?? (status.outputs.device ? `On ${status.outputs.device}` : 'No speakers open'),
      href: '#/admin/audio',
    })
    for (const o of status.outputs.outputs) {
      if (o.state === 'disabled' || o.clock) continue
      const off = Math.abs(o.offset_ms)
      lines.push({
        level: o.error ? 'bad' : off > 20 ? 'warn' : 'ok',
        label: o.device ?? 'Speaker',
        detail: o.error ?? `In time with the main speaker (${o.offset_ms} ms, ${o.drift_ppm} ppm)`,
        href: '#/admin/audio',
      })
    }
    for (const cast of status.casts) {
      lines.push({
        level: cast.error ? 'bad' : cast.playing ? 'ok' : 'warn',
        label: cast.name,
        detail: cast.error ?? (cast.playing ? 'Casting' : 'Connecting'),
        href: '#/admin/audio',
      })
    }
    return lines
  })

  const computer = $derived.by((): Line[] => {
    if (!status) return []
    const lines: Line[] = []
    if (status.cpu !== null) {
      const pct = Math.round(status.cpu * 100)
      lines.push({ level: pct >= 90 ? 'warn' : 'ok', label: 'Processor', detail: `${pct}% busy` })
    }
    if (status.memory_total && status.memory_available !== null) {
      const used = 1 - status.memory_available / status.memory_total
      lines.push({
        level: used >= 0.92 ? 'warn' : 'ok',
        label: 'Memory',
        detail: `${size(status.memory_available)} free of ${size(status.memory_total)}`,
      })
    }
    if (status.disk_total && status.disk_available !== null) {
      const free = status.disk_available
      lines.push({
        level: free < 1e9 ? 'bad' : free < 5e9 || free / status.disk_total < 0.05 ? 'warn' : 'ok',
        label: 'Disk',
        detail: `${size(free)} free where DJINN keeps its data`,
        href: '#/admin/system',
      })
    }
    if (status.public_network) {
      lines.push({
        level: 'bad',
        label: 'Network',
        detail: 'Windows has this network set to Public, which keeps phones out',
        href: '#/admin/home',
      })
    }
    return lines
  })

  const library = $derived.by((): Line[] => {
    if (!status) return []
    const lines: Line[] = [
      {
        level: status.library_folders ? 'ok' : 'bad',
        label: 'Music folders',
        detail: status.scanning
          ? 'Checking for changes now'
          : `${status.library_folders} ${status.library_folders === 1 ? 'folder' : 'folders'}`,
        href: '#/admin/library',
      },
    ]
    if (problems) {
      lines.push({
        level: problems.trouble ? 'warn' : 'ok',
        label: 'Problems',
        detail: problems.trouble ? `${n(problems.trouble)} need a look` : 'Nothing needs a look',
        href: '#/admin/problems',
      })
    }
    if (lock?.recovery_used) {
      lines.push({
        level: 'bad',
        label: 'Recovery used',
        detail: `The host's recovery command was used for ${lock.recovery_used.username} on ${new Date(lock.recovery_used.at).toLocaleDateString()}`,
        href: '#/admin/protected',
      })
    }
    if (lock && lock.level === 'off') {
      lines.push({
        level: 'warn',
        label: 'Protected Areas',
        detail: 'The lock on library changes is off',
        href: '#/admin/protected',
      })
    }
    if (lyricsImport?.warning_count) {
      lines.push({
        level: 'warn',
        label: 'Lyrics import',
        detail: `${n(lyricsImport.warning_count)} ${lyricsImport.warning_count === 1 ? 'file' : 'files'} thrown out for looking like code`,
        href: '#/admin/library',
      })
    }
    if (nightly?.on && nightly.last) {
      lines.push({
        level: 'ok',
        label: 'Nightly check',
        detail: `Last ran ${ago(nightly.last.at)}: ${n(nightly.last.added)} added, ${n(nightly.last.removed)} gone`,
        href: '#/admin/problems',
      })
    }
    return lines
  })

  const upkeep = $derived.by((): Line[] => {
    if (!status) return []
    const backup = status.last_backup_ms
    const lines: Line[] = [
      {
        level: !backup ? 'warn' : Date.now() - backup > 30 * 86_400_000 ? 'warn' : 'ok',
        label: 'Backup',
        detail: backup ? `Last one ${ago(backup)}` : 'No backup yet: save one so settings and playlists are safe',
        href: '#/admin/system',
      },
    ]
    if (about) {
      lines.push({
        level: about.update_available ? 'warn' : about.error ? 'warn' : 'ok',
        label: 'Updates',
        detail: about.update_available
          ? `DJINN ${about.latest?.version ?? ''} is ready to install`
          : about.error
            ? "Couldn't check for updates"
            : `Up to date (${about.version})`,
        href: '#/admin/system',
      })
    }
    return lines
  })
</script>

{#snippet block(title: string, lines: Line[], icon: 'playing' | 'library' | 'settings' | 'cloud')}
  {#if lines.length}
    <section class="card">
      <h2><Icon name={icon} size={20} /> {title}</h2>
      <ul>
        {#each lines as line, i (i)}
          <li>
            <span class="dot {line.level}" aria-label={line.level === 'ok' ? 'Fine' : line.level === 'warn' ? 'Worth a look' : 'Wrong'}
            ></span>
            <div class="grow">
              {#if line.href}<a href={line.href}>{line.label}</a>{:else}<span>{line.label}</span>{/if}
              <div class="small muted">{line.detail}</div>
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
{/snippet}

{#if !status}
  <p class="small muted">Loading...</p>
{:else}
  <div class="stats">
    <div class="stat">
      <Icon name="library" size={22} />
      <strong>{n(status.songs)}</strong>
      <span class="small muted">Songs</span>
    </div>
    <div class="stat">
      <Icon name="person" size={22} />
      <strong>{n(status.artists)}</strong>
      <span class="small muted">Artists</span>
    </div>
    <div class="stat">
      <Icon name="playlist" size={22} />
      <strong>{n(status.albums)}</strong>
      <span class="small muted">Albums</span>
    </div>
    <div class="stat">
      <Icon name="clock" size={22} />
      <strong>{hoursOf(status.library_ms)}</strong>
      <span class="small muted">Of music{days(status.library_ms)}</span>
    </div>
    <div class="stat">
      <Icon name="folder" size={22} />
      <strong>{size(status.library_bytes)}</strong>
      <span class="small muted">Music on disk</span>
    </div>
  </div>
  {#if listening && listening.sum.plays}
    {@const l = listening}
    <section class="card all-time">
      <h2><Icon name="headphones" size={20} /> All-time listening</h2>
      <p class="small muted">
        Party and DJINN GO together{#if l.since}, since {l.since}{/if}. More in
        <a href="#/admin/stats">Stats</a>.
      </p>
      <div class="stats">
        <div class="stat">
          <strong>{hoursOf(l.sum.listened_ms)}</strong>
          <span class="small muted">Listened{days(l.sum.listened_ms)}</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.plays)}</strong>
          <span class="small muted">Plays</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.songs)}</strong>
          <span class="small muted">Different songs ({l.share}% of the library)</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.requests)}</strong>
          <span class="small muted">Requests, from {n(l.sum.requesters)} {l.sum.requesters === 1 ? 'person' : 'people'}</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.skips)}</strong>
          <span class="small muted">Skips</span>
        </div>
      </div>
      <ul>
        {#if l.song}
          <li>
            <div class="grow">
              <span>Most played song</span>
              <div class="small muted">
                {l.song.title}{#if l.song.artist}, {l.song.artist}{/if} ({n(l.song.plays)} plays)
              </div>
            </div>
          </li>
        {/if}
        {#if l.artist}
          <li>
            <div class="grow">
              <span>Most played artist</span>
              <div class="small muted">{l.artist.name} ({n(l.artist.plays)} plays)</div>
            </div>
          </li>
        {/if}
        {#if l.requested}
          <li>
            <div class="grow">
              <span>Most requested</span>
              <div class="small muted">
                {l.requested.title ?? 'A removed song'}{#if l.requested.artist}, {l.requested.artist}{/if}
                ({n(l.requested.requests)} requests)
              </div>
            </div>
          </li>
        {/if}
        {#if l.weekday || l.hour}
          <li>
            <div class="grow">
              <span>Busiest time</span>
              <div class="small muted">
                {[l.weekday, l.hour ? `around ${l.hour}` : null].filter(Boolean).join(', ')}
              </div>
            </div>
          </li>
        {/if}
      </ul>
    </section>
  {/if}
  <div class="cards-2">
    {@render block('Music', music, 'playing')}
    {@render block('Library', library, 'library')}
    {@render block('This computer', computer, 'settings')}
    {@render block('Upkeep', upkeep, 'cloud')}
  </div>
{/if}

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: 12px;
    margin-bottom: 16px;
  }

  .stat {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 16px;
    border: 1px solid var(--edge);
    border-radius: 16px;
    background: var(--card-bg);
    box-shadow: var(--glow);
    color: var(--accent);
  }

  .all-time {
    margin-bottom: 16px;
  }

  .all-time .stats {
    margin: 10px 0;
  }

  .all-time .stat {
    box-shadow: none;
  }

  .stat strong {
    color: var(--text);
    font-size: 1.5rem;
    line-height: 1.2;
  }

  h2 {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--line);
  }

  li:first-child {
    border-top: 0;
  }

  .grow {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    margin-top: 6px;
    border-radius: 50%;
  }

  .dot.ok {
    background: var(--ok);
  }

  .dot.warn {
    background: #e0a100;
  }

  .dot.bad {
    background: var(--bad);
  }
</style>
