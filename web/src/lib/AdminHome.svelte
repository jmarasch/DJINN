<script lang="ts">
  // Admin > Home: how DJINN is doing at a glance. Each line says whether
  // it's fine, worth a look, or wrong, and links to where to fix it.
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { toast } from './state.svelte'
  import type { About, ProblemSummary, ScheduledCheck, SystemStatus } from './types'

  type Level = 'ok' | 'warn' | 'bad'
  type Line = { level: Level; label: string; detail: string; href?: string }

  let status = $state<SystemStatus | null>(null)
  let problems = $state<ProblemSummary | null>(null)
  let nightly = $state<ScheduledCheck | null>(null)
  let about = $state<About | null>(null)

  async function load() {
    try {
      status = await api.systemStatus()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the status', true)
    }
  }

  $effect(() => {
    void load()
    api.libraryProblems().then((p) => (problems = p)).catch(() => {})
    api.scheduledCheck().then((c) => (nightly = c)).catch(() => {})
    api.about().then((a) => (about = a)).catch(() => {})
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
      <Icon name="folder" size={22} />
      <strong>{size(status.library_bytes)}</strong>
      <span class="small muted">Music on disk</span>
    </div>
  </div>
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
