<script lang="ts">
  // Admin > Home: how DJINN is doing at a glance. Each line says whether
  // it's fine, worth a look, or wrong, and links to where to fix it.
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { locale, t } from './i18n.svelte'
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
      toast(e instanceof Error ? e.message : t('admin.home.load_failed'), true)
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

  const n = (x: number) => x.toLocaleString(locale.code)
  const size = (bytes: number) =>
    bytes >= 1e12
      ? `${one(bytes / 1e12)} TB`
      : bytes >= 1e9
        ? `${one(bytes / 1e9)} GB`
        : `${n(Math.round(bytes / 1e6))} MB`
  /** A number with one decimal, in the language's own way. */
  const one = (x: number) => x.toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })
  /** A time in whole hours, or minutes when under one. */
  const hoursOf = (ms: number) =>
    ms >= 3_600_000
      ? t('admin.home.hours', { n: n(Math.floor(ms / 3_600_000)) })
      : t('admin.home.minutes', { n: Math.floor(ms / 60_000) })
  /** The same as days and hours, for two days or more. */
  const days = (ms: number) => {
    const hours = Math.floor(ms / 3_600_000)
    return hours >= 48 ? ` (${t('admin.home.days_hours', { days: n(Math.floor(hours / 24)), hours: hours % 24 })})` : ''
  }
  const hourName = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString(locale.code, { hour: 'numeric' })
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
      weekday: weekday && weekday[1] ? t(`admin.home.weekday.${weekday[0]}`) : null,
      since: first ? new Date(`${first}T12:00:00`).toLocaleDateString(locale.code, { year: 'numeric', month: 'long', day: 'numeric' }) : null,
    }
  })

  const ago = (ms: number) => {
    const days = Math.floor((Date.now() - ms) / 86_400_000)
    return days < 1 ? t('admin.home.today') : days === 1 ? t('admin.home.yesterday') : t('admin.home.days_ago', { n: days })
  }

  const music = $derived.by((): Line[] => {
    if (!status) return []
    const lines: Line[] = []
    const state = { idle: t('admin.home.nothing_playing'), playing: t('admin.home.playing'), paused: t('admin.home.paused') }[
      status.state
    ]
    const playing = status.outputs.outputs.filter((o) => o.state === 'playing')
    lines.push({
      level: status.outputs.error ? 'bad' : playing.length ? 'ok' : 'warn',
      label: state,
      detail: status.outputs.error ?? (status.outputs.device ? t('admin.home.on_device', { device: status.outputs.device }) : t('admin.home.no_speakers')),
      href: '#/admin/audio',
    })
    for (const o of status.outputs.outputs) {
      if (o.state === 'disabled' || o.clock) continue
      const off = Math.abs(o.offset_ms)
      lines.push({
        level: o.error ? 'bad' : off > 20 ? 'warn' : 'ok',
        label: o.device ?? t('admin.home.speaker'),
        detail: o.error ?? t('admin.home.in_time', { ms: o.offset_ms, ppm: o.drift_ppm }),
        href: '#/admin/audio',
      })
    }
    for (const cast of status.casts) {
      lines.push({
        level: cast.error ? 'bad' : cast.playing ? 'ok' : 'warn',
        label: cast.name,
        detail: cast.error ?? (cast.playing ? t('admin.home.casting') : t('admin.home.connecting')),
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
      lines.push({ level: pct >= 90 ? 'warn' : 'ok', label: t('admin.home.processor'), detail: t('admin.home.busy', { pct }) })
    }
    if (status.memory_total && status.memory_available !== null) {
      const used = 1 - status.memory_available / status.memory_total
      lines.push({
        level: used >= 0.92 ? 'warn' : 'ok',
        label: t('admin.home.memory'),
        detail: t('admin.home.free_of', { free: size(status.memory_available), total: size(status.memory_total) }),
      })
    }
    if (status.disk_total && status.disk_available !== null) {
      const free = status.disk_available
      lines.push({
        level: free < 1e9 ? 'bad' : free < 5e9 || free / status.disk_total < 0.05 ? 'warn' : 'ok',
        label: t('admin.home.disk'),
        detail: t('admin.home.disk_free', { free: size(free) }),
        href: '#/admin/system',
      })
    }
    if (status.public_network) {
      lines.push({
        level: 'bad',
        label: t('admin.home.network'),
        detail: t('admin.home.public_network'),
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
        label: t('admin.home.folders'),
        detail: status.scanning
          ? t('admin.home.checking')
          : t('admin.home.n_folders', { n: status.library_folders }),
        href: '#/admin/library',
      },
    ]
    if (problems) {
      lines.push({
        level: problems.trouble ? 'warn' : 'ok',
        label: t('admin.home.problems'),
        detail: problems.trouble ? t('admin.home.need_a_look', { n: problems.trouble }) : t('admin.home.nothing_to_look'),
        href: '#/admin/problems',
      })
    }
    if (lock?.recovery_used) {
      lines.push({
        level: 'bad',
        label: t('admin.home.recovery_used'),
        detail: t('admin.home.recovery_detail', {
          name: lock.recovery_used.username,
          date: new Date(lock.recovery_used.at).toLocaleDateString(locale.code),
        }),
        href: '#/admin/protected',
      })
    }
    if (lock && lock.level === 'off') {
      lines.push({
        level: 'warn',
        label: t('admin.menu.protected'),
        detail: t('admin.home.lock_off'),
        href: '#/admin/protected',
      })
    }
    if (lyricsImport?.warning_count) {
      lines.push({
        level: 'warn',
        label: t('admin.home.lyrics_import'),
        detail: t('admin.home.thrown_out', { n: lyricsImport.warning_count }),
        href: '#/admin/library',
      })
    }
    if (nightly?.on && nightly.last) {
      lines.push({
        level: 'ok',
        label: t('admin.home.nightly'),
        detail: t('admin.home.nightly_detail', {
          when: ago(nightly.last.at),
          added: n(nightly.last.added),
          removed: n(nightly.last.removed),
        }),
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
        label: t('admin.home.backup'),
        detail: backup ? t('admin.home.last_backup', { when: ago(backup) }) : t('admin.home.no_backup'),
        href: '#/admin/system',
      },
    ]
    if (about) {
      lines.push({
        level: about.update_available ? 'warn' : about.error ? 'warn' : 'ok',
        label: t('admin.home.updates'),
        detail: about.update_available
          ? t('admin.home.update_ready', { version: about.latest?.version ?? '' })
          : about.error
            ? t('admin.home.update_failed')
            : t('admin.home.up_to_date', { version: about.version }),
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
            <span class="dot {line.level}" aria-label={line.level === 'ok'
                ? t('admin.home.fine')
                : line.level === 'warn'
                  ? t('admin.home.worth_a_look')
                  : t('admin.home.wrong')}
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
  <p class="small muted">{t('common.loading')}</p>
{:else}
  <div class="stats">
    <div class="stat">
      <Icon name="library" size={22} />
      <strong>{n(status.songs)}</strong>
      <span class="small muted">{t('admin.home.songs')}</span>
    </div>
    <div class="stat">
      <Icon name="person" size={22} />
      <strong>{n(status.artists)}</strong>
      <span class="small muted">{t('admin.home.artists')}</span>
    </div>
    <div class="stat">
      <Icon name="playlist" size={22} />
      <strong>{n(status.albums)}</strong>
      <span class="small muted">{t('admin.home.albums')}</span>
    </div>
    <div class="stat">
      <Icon name="clock" size={22} />
      <strong>{hoursOf(status.library_ms)}</strong>
      <span class="small muted">{t('admin.home.of_music')}{days(status.library_ms)}</span>
    </div>
    <div class="stat">
      <Icon name="folder" size={22} />
      <strong>{size(status.library_bytes)}</strong>
      <span class="small muted">{t('admin.home.on_disk')}</span>
    </div>
  </div>
  {#if listening && listening.sum.plays}
    {@const l = listening}
    <section class="card all-time">
      <h2><Icon name="headphones" size={20} /> {t('admin.home.all_time')}</h2>
      <p class="small muted">
        {t('admin.home.all_time_note')}{#if l.since}, {t('admin.home.since', { date: l.since })}{/if}. {t(
          'admin.home.more_in',
        )}
        <a href="#/admin/stats">{t('admin.menu.stats')}</a>.
      </p>
      <div class="stats">
        <div class="stat">
          <strong>{hoursOf(l.sum.listened_ms)}</strong>
          <span class="small muted">{t('admin.home.listened')}{days(l.sum.listened_ms)}</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.plays)}</strong>
          <span class="small muted">{t('admin.home.plays')}</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.songs)}</strong>
          <span class="small muted">{t('admin.home.different_songs', { pct: l.share })}</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.requests)}</strong>
          <span class="small muted">{t('admin.home.requests_from', { n: l.sum.requesters })}</span>
        </div>
        <div class="stat">
          <strong>{n(l.sum.skips)}</strong>
          <span class="small muted">{t('admin.home.skips')}</span>
        </div>
      </div>
      <ul>
        {#if l.song}
          <li>
            <div class="grow">
              <span>{t('admin.home.top_song')}</span>
              <div class="small muted">
                {l.song.title}{#if l.song.artist}, {l.song.artist}{/if} ({t('admin.home.n_plays', { n: l.song.plays })})
              </div>
            </div>
          </li>
        {/if}
        {#if l.artist}
          <li>
            <div class="grow">
              <span>{t('admin.home.top_artist')}</span>
              <div class="small muted">{l.artist.name} ({t('admin.home.n_plays', { n: l.artist.plays })})</div>
            </div>
          </li>
        {/if}
        {#if l.requested}
          <li>
            <div class="grow">
              <span>{t('admin.home.top_requested')}</span>
              <div class="small muted">
                {l.requested.title ?? t('admin.home.removed_song')}{#if l.requested.artist}, {l.requested.artist}{/if}
                ({t('admin.home.n_requests', { n: l.requested.requests })})
              </div>
            </div>
          </li>
        {/if}
        {#if l.weekday || l.hour}
          <li>
            <div class="grow">
              <span>{t('admin.home.busiest')}</span>
              <div class="small muted">
                {[l.weekday, l.hour ? t('admin.home.around', { time: l.hour }) : null].filter(Boolean).join(', ')}
              </div>
            </div>
          </li>
        {/if}
      </ul>
    </section>
  {/if}
  <div class="cards-2">
    {@render block(t('admin.home.music'), music, 'playing')}
    {@render block(t('admin.home.library'), library, 'library')}
    {@render block(t('admin.home.computer'), computer, 'settings')}
    {@render block(t('admin.home.upkeep'), upkeep, 'cloud')}
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
