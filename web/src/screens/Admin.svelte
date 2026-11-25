<script lang="ts">
  import { api } from '../lib/api'
  import FolderPicker from '../lib/FolderPicker.svelte'
  import Icon from '../lib/Icon.svelte'
  import AboutPanel from '../lib/AboutPanel.svelte'
  import BackupPanel from '../lib/BackupPanel.svelte'
  import LibraryExtras from '../lib/LibraryExtras.svelte'
  import LibraryProgress from '../lib/LibraryProgress.svelte'
  import LibraryProblems from '../lib/LibraryProblems.svelte'
  import LibraryDuplicates from '../lib/LibraryDuplicates.svelte'
  import ScheduledCheck from '../lib/ScheduledCheck.svelte'
  import { clearListCache } from '../lib/listCache'
  import LogViewer from '../lib/LogViewer.svelte'
  import SignInLog from '../lib/SignInLog.svelte'
  import ProtectedPanel from '../lib/ProtectedPanel.svelte'
  import AiWorkPanel from '../lib/AiWorkPanel.svelte'
  import LibraryEditor from '../lib/LibraryEditor.svelte'
  import UnlockHost from '../lib/UnlockHost.svelte'
  import ResetPanel from '../lib/ResetPanel.svelte'
  import SystemStatus from '../lib/SystemStatus.svelte'
  import UpdatePanel from '../lib/UpdatePanel.svelte'
  import AiPanel from '../lib/AiPanel.svelte'
  import GenreGroupsPanel from '../lib/GenreGroupsPanel.svelte'
  import GenreWebPanel from '../lib/GenreWebPanel.svelte'
  import GenreMapPanel from '../lib/GenreMapPanel.svelte'
  import CastPanel from '../lib/CastPanel.svelte'
  import CrossfadePanel from '../lib/CrossfadePanel.svelte'
  import EqualizerPanel from '../lib/EqualizerPanel.svelte'
  import OutputsPanel from '../lib/OutputsPanel.svelte'
  import PublicNetworkNote from '../lib/PublicNetworkNote.svelte'
  import ReplayGainPanel from '../lib/ReplayGainPanel.svelte'
  import SmartPlayPanel from '../lib/SmartPlayPanel.svelte'
  import SchedulePanel from '../lib/SchedulePanel.svelte'
  import PageTitle from '../lib/PageTitle.svelte'
  import { adminGroups, adminSubtabs, adminTabLabel, type AdminTab } from '../lib/adminTabs'
  import { adminEntries, entryLabel, searchAdmin, type AdminEntry } from '../lib/adminSearch'
  import AdminHome from '../lib/AdminHome.svelte'
  import GuidePanel from '../lib/GuidePanel.svelte'
  import StatsPanel from '../lib/StatsPanel.svelte'
  import SearchesWishes from '../lib/SearchesWishes.svelte'
  import { tick } from 'svelte'
  import Sheet from '../lib/Sheet.svelte'
  import UsersPanel from '../lib/UsersPanel.svelte'
  import GoPanel from '../lib/GoPanel.svelte'
  import NodesPanel from '../lib/NodesPanel.svelte'
  import VisualsPanel from '../lib/VisualsPanel.svelte'
  import HouseLanguage from '../lib/HouseLanguage.svelte'
  import AppearancePanel from '../lib/AppearancePanel.svelte'
  import RoomsPanel from '../lib/RoomsPanel.svelte'
  import { app, toast } from '../lib/state.svelte'
  import { locale, t } from '../lib/i18n.svelte'
  import type { LibraryRoot, RequestLogEntry, Settings } from '../lib/types'

  let { tab = 'home' }: { tab?: AdminTab } = $props()

  // Two layouts: one section at a time, or every section on one page with
  // the menu jumping to each section and card. Only sections are offered for
  // now; set this to bring the one-page layout and its switch back.
  const OFFER_ONE_PAGE = false
  type Layout = 'sections' | 'page'
  let layout = $state<Layout>(OFFER_ONE_PAGE ? storedLayout() : 'sections')
  const allTabs: AdminTab[] = adminGroups.flatMap((g) => g.tabs.map((x) => x.id))

  function storedLayout(): Layout {
    try {
      return localStorage.getItem('djinn-admin-layout') === 'page' ? 'page' : 'sections'
    } catch {
      return 'sections'
    }
  }

  function setLayout(next: Layout) {
    layout = next
    try {
      localStorage.setItem('djinn-admin-layout', next)
    } catch {
      // Only a convenience.
    }
    if (next === 'page') void loadAll()
  }

  // On one page: each section's cards for the menu, read off the page, and
  // the section in view.
  let content = $state<HTMLElement>()
  let cards = $state<Record<string, { title: string; el: HTMLElement }[]>>({})
  let inView = $state<AdminTab>('home')

  function readCards() {
    if (!content) return
    const found: Record<string, { title: string; el: HTMLElement }[]> = {}
    for (const section of content.querySelectorAll<HTMLElement>('.admin-section')) {
      found[section.dataset.tab ?? ''] = [...section.querySelectorAll<HTMLElement>('section.card')]
        .map((el) => ({ title: el.querySelector('h2, h3')?.textContent?.trim() ?? '', el }))
        .filter((c) => c.title)
    }
    cards = found
  }

  $effect(() => {
    if (layout !== 'page' || !content) return
    let timer: ReturnType<typeof setTimeout> | undefined
    const observer = new MutationObserver(() => {
      clearTimeout(timer)
      timer = setTimeout(readCards, 300)
    })
    observer.observe(content, { childList: true, subtree: true })
    readCards()
    const onScroll = () => {
      let current: AdminTab = 'home'
      for (const section of content?.querySelectorAll<HTMLElement>('.admin-section') ?? []) {
        if (section.getBoundingClientRect().top < 140) current = section.dataset.tab as AdminTab
      }
      inView = current
    }
    window.addEventListener('scroll', onScroll, { passive: true })
    onScroll()
    return () => {
      observer.disconnect()
      clearTimeout(timer)
      window.removeEventListener('scroll', onScroll)
    }
  })

  function jump(el: Element | null | undefined) {
    el?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  // A link to a section (#/admin/rules) scrolls to it on one page. The
  // sections above it are still loading and grow, so it follows the section
  // until the page settles, unless you scroll yourself.
  $effect(() => {
    const target = tab
    if (layout !== 'page' || target === 'home') return
    let stopped = false
    const stop = () => (stopped = true)
    window.addEventListener('wheel', stop, { once: true, passive: true })
    window.addEventListener('touchmove', stop, { once: true, passive: true })
    const timers = [0, 400, 1000, 2000, 3500].map((ms) =>
      setTimeout(() => {
        if (stopped) return
        const el = document.getElementById(`admin-${target}`)
        el?.scrollIntoView({ block: 'start' })
      }, ms),
    )
    return () => {
      timers.forEach(clearTimeout)
      window.removeEventListener('wheel', stop)
      window.removeEventListener('touchmove', stop)
    }
  })

  async function loadAll() {
    await Promise.all([load('library'), load('log')]).catch((e) => toast(e.message, true))
  }

  // Finding a setting: what's typed, the setting to highlight once its
  // section shows, settings changed from their defaults, and problems.
  let query = $state('')
  let finding = $state<AdminEntry | null>(null)
  let changed = $state<string[]>([])
  let showChanged = $state(false)
  let trouble = $state(0)
  const results = $derived(searchAdmin(query))
  const changedEntries = $derived(adminEntries.filter((e) => e.key && changed.includes(e.key)))
  const changedTabs = $derived(new Set(changedEntries.map((e) => e.tab)))

  $effect(() => {
    void tab
    api.changedSettings().then((c) => (changed = c)).catch(() => {})
    api.libraryProblems(true).then((p) => (trouble = p.trouble)).catch(() => {})
  })

  function find(entry: AdminEntry) {
    query = ''
    showChanged = false
    finding = entry
    if (layout === 'page') void highlight(entry)
    else if (tab !== entry.tab) location.hash = `#/admin/${entry.tab}`
    else void highlight(entry)
  }

  $effect(() => {
    if (finding && (layout === 'page' || finding.tab === tab)) void highlight(finding)
  })

  /** Scrolls to the setting's text on the page and flashes it. Sections
   * load their data, so this looks for a few seconds. */
  async function highlight(entry: AdminEntry) {
    finding = null
    await tick()
    const want = entryLabel(entry).toLowerCase().slice(0, 40)
    for (let i = 0; i < 30; i++) {
      const found = [...document.querySelectorAll<HTMLElement>('.admin-content :is(h2, h3, label, p, button, dt, a)')]
        .filter((el) => (el.textContent ?? '').toLowerCase().includes(want))
        .sort((a, b) => (a.textContent ?? '').length - (b.textContent ?? '').length)[0]
      if (found) {
        const holder = found.closest<HTMLElement>('[data-subtab]')
        if (holder?.hidden) {
          const group = holder.closest('.admin-content')?.querySelector('.subtabs')
            ? Object.keys(sub).find((g) => entry.tab === g)
            : undefined
          if (group && holder.dataset.subtab) {
            openSub(group, holder.dataset.subtab)
            await tick()
          }
        }
        found.scrollIntoView({ behavior: 'smooth', block: 'center' })
        found.classList.add('found')
        setTimeout(() => found.classList.remove('found'), 2500)
        return
      }
      await new Promise((r) => setTimeout(r, 100))
    }
  }

  // Tabs inside the longest sections; the others stay in the page, hidden,
  // so Find a setting can still reach them.
  let sub = $state<Record<string, string>>({ library: 'folders', genres: 'map', smart: 'play', audio: 'sound', ai: 'search' })

  function openSub(group: string, id: string) {
    sub[group] = id
  }

  let roots = $state<LibraryRoot[]>([])
  let trackCount = $state(0)
  let artDir = $state('')
  let log = $state<RequestLogEntry[]>([])
  let settings = $state<Settings | null>(null)
  let picking = $state(false)

  $effect(() => {
    if (layout === 'page') void loadAll()
    else load(tab).catch((e) => toast(e.message, true))
  })

  async function load(which: AdminTab) {
    if (which === 'home' || which === 'problems' || which === 'stats') {
      // These load their own data.
    } else if (which === 'library') {
      const [r, status, s, rg] = await Promise.all([
        api.libraryRoots(),
        api.libraryStatus(),
        api.settings(),
        api.replayGain(),
      ])
      app.loudness = rg.loudness
      roots = r
      trackCount = status.tracks
      artDir = status.art_dir
      settings = s
      if (status.last_scan) app.scan = status.last_scan
    } else if (which === 'audio' || which === 'smart' || which === 'users' || which === 'go') {
      // These panels load their own data.
    } else if (which === 'log') {
      log = await api.requestLog(200)
    } else {
      settings = await api.settings()
    }
  }

  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await action()
      if (done) toast(done)
      return true
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
      return false
    }
  }

  async function addRoot(path: string) {
    picking = false
    await attempt(async () => (roots = await api.addLibraryRoot(path)), t('admin.folder_added'))
  }

  async function removeRoot(root: LibraryRoot) {
    if (!confirm(t('admin.remove_folder_q', { path: root.path }))) return
    await attempt(async () => (roots = await api.removeLibraryRoot(root.path)), t('admin.folder_removed'))
  }

  async function checkForChanges() {
    try {
      const { status } = await api.rescan()
      toast(status === 'started' ? t('admin.checking') : t('admin.already_checking'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('admin.check_failed'), true)
    }
  }

  async function clearLog() {
    if (!confirm(t('admin.clear_log_q'))) return
    if (await attempt(() => api.clearRequestLog(), t('admin.log_cleared'))) log = []
  }

  async function saveSettings(e: SubmitEvent) {
    e.preventDefault()
    if (settings && await attempt(() => api.saveSettings(settings!), t('common.saved'))) {
      clearListCache()
      api.changedSettings().then((c) => (changed = c)).catch(() => {})
    }
  }

  const scanning = $derived(app.scan !== null && !app.scan.done)
  const time = (ms: number) => new Date(ms).toLocaleTimeString(locale.code, { hour: 'numeric', minute: '2-digit' })
  const when = (ms: number) =>
    new Date(ms).toLocaleString(locale.code, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

{#snippet subtabs(group: string, items: [string, string][])}
  <div class="subtabs" role="tablist">
    {#each items as [id, label] (id)}
      <button role="tab" aria-selected={sub[group] === id} class:active={sub[group] === id} onclick={() => openSub(group, id)}>
        {label}
      </button>
    {/each}
  </div>
{/snippet}

{#snippet sectionContent(id: AdminTab)}
{#if id === 'home'}
  <AdminHome />
{:else if id === 'guide'}
  <GuidePanel />
{:else if id === 'stats'}
  <StatsPanel />
  <SearchesWishes />
{:else if id === 'problems'}
  <ScheduledCheck />
  <LibraryProblems />
  <LibraryDuplicates />
{:else if id === 'library'}
  {@render subtabs('library', adminSubtabs('library'))}
  <div data-subtab="folders" hidden={sub.library !== 'folders'}>
  <div class="cards-2">
  <section class="card">
    <h2>{t('admin.music_folders')}</h2>
    {#each roots as root (root.path)}
      <div class="item">
        <Icon name="folder" size={20} />
        <div class="grow">
          <div class="path">{root.path}</div>
          {#if !root.available}<div class="small error">{t('admin.not_found_now')}</div>{/if}
          {#if root.from_config}<div class="small muted">{t('admin.from_config')}</div>{/if}
        </div>
        {#if !root.from_config}
          <button class="icon" aria-label={t('admin.remove_path', { path: root.path })} onclick={() => removeRoot(root)}><Icon name="close" /></button>
        {/if}
      </div>
    {:else}
      <p class="muted">{t('admin.no_folders')}</p>
    {/each}
    <button class="primary" onclick={() => (picking = true)}>{t('admin.add_folder')}</button>
  </section>

  <section class="card">
    <h2>{t('admin.library')}</h2>
    <p class="muted">{t('count.songs', { n: trackCount })}</p>
    <LibraryProgress />
    {#if app.scan && !scanning}
      <p class="small muted">{t('admin.last_check', { found: app.scan.found, read: app.scan.read, removed: app.scan.removed })}</p>
    {/if}
    <button disabled={scanning} onclick={checkForChanges}>{t('admin.check_changes')}</button>
  </section>

  <section class="card">
    <h2>{t('admin.image_cache')}</h2>
    <p class="small muted">
      {t('admin.image_cache_note')}
    </p>
    <p class="path small mono">{artDir}</p>
    <button onclick={() => attempt(api.openArtFolder)}>{t('admin.open_image_folder')}</button>
  </section>
  </div>
  <p class="small muted">{t('admin.problems_note')} <a href="#/admin/problems">{t('admin.problems_link')}</a>.</p>
  </div>

  <div data-subtab="settings" hidden={sub.library !== 'settings'}>
  {#if settings}
    <section class="card wide">
      <h2>{t('admin.artist_library')}</h2>
      <form class="settings" onsubmit={saveSettings}>
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.show_collaboration_artists} />
          {t('admin.show_credited')}
        </label>
        <p class="small muted">{t('admin.show_credited_note')}</p>
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.original_years} />
          {t('admin.original_years')}
        </label>
        <p class="small muted">{t('admin.original_years_note')}</p>
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.album_folders} />
          {t('admin.album_folders')}
        </label>
        <p class="small muted">{t('admin.album_folders_note')}</p>
        <label>
          {t('admin.genre_page_size')}
          <input type="number" min="10" max="500" bind:value={settings.genre_page_size} />
        </label>
        <label>
          {t('admin.genre_refresh')}
          <input type="number" min="0" max="8760" bind:value={settings.genre_refresh_hours} />
          <span class="small muted">{t('admin.genre_refresh_note')}</span>
        </label>
        <label>
          {t('admin.discover_refresh')}
          <select bind:value={settings.discover_refresh_minutes}>
            <option value={0}>{t('admin.every_open')}</option>
            <option value={1}>{t('admin.every_minute')}</option>
            <option value={5}>{t('admin.every_minutes', { n: 5 })}</option>
            <option value={15}>{t('admin.every_minutes', { n: 15 })}</option>
            <option value={60}>{t('admin.every_hour')}</option>
            <option value={1440}>{t('admin.every_day')}</option>
          </select>
          <span class="small muted">{t('admin.discover_refresh_note')}</span>
        </label>
        <label>
          {t('admin.play_count', { pct: settings.play_count_percent })}
          <input type="range" min="0" max="100" step="5" bind:value={settings.play_count_percent} />
          <span class="small muted">{t('admin.play_count_note')}</span>
        </label>
        <label>
          {t('admin.keep_whole')}
          <textarea rows="6" bind:value={settings.artist_keep_whole} spellcheck="false"></textarea>
          <span class="small muted">
            {t('admin.keep_whole_note')}
          </span>
        </label>
        <button class="primary">{t('common.save')}</button>
      </form>
    </section>
  {/if}
  </div>

  <div data-subtab="online" hidden={sub.library !== 'online'}>
  {#if settings}
    <div class="cards-2"><LibraryExtras bind:settings /></div>
  {/if}
  </div>


  {#if picking}
    <Sheet title={t('admin.pick_folder')} onclose={() => (picking = false)}>
      <FolderPicker onpick={addRoot} />
    </Sheet>
  {/if}
{:else if id === 'genres'}
  {@render subtabs('genres', adminSubtabs('genres'))}
  <div data-subtab="map" hidden={sub.genres !== 'map'}><div class="cards-2"><GenreMapPanel /></div></div>
  <div data-subtab="tree" hidden={sub.genres !== 'tree'}><div class="cards-2"><GenreWebPanel /></div></div>
  <div data-subtab="containers" hidden={sub.genres !== 'containers'}>
    <div class="cards-2"><GenreGroupsPanel /></div>
  </div>
{:else if id === 'audio'}
  {@render subtabs('audio', adminSubtabs('audio'))}
  <div data-subtab="sound" hidden={sub.audio !== 'sound'}>
    <div class="cards-2">
      <div class="wide"><EqualizerPanel /></div>
      <CrossfadePanel />
      <ReplayGainPanel />
    </div>
  </div>
  <div data-subtab="outputs" hidden={sub.audio !== 'outputs'}><OutputsPanel /></div>
  <div data-subtab="casting" hidden={sub.audio !== 'casting'}><CastPanel /></div>
{:else if id === 'smart'}
  {@render subtabs('smart', adminSubtabs('smart'))}
  <div data-subtab="play" hidden={sub.smart !== 'play'}><div class="cards-2"><SmartPlayPanel part="play" /></div></div>
  <div data-subtab="styles" hidden={sub.smart !== 'styles'}><SmartPlayPanel part="styles" /></div>
  <div data-subtab="schedule" hidden={sub.smart !== 'schedule'}><SchedulePanel /></div>
{:else if id === 'ai'}
  {@render subtabs('ai', adminSubtabs('ai'))}
  <div data-subtab="search" hidden={sub.ai !== 'search'}><div class="cards-2"><AiPanel part="search" /></div></div>
  <div data-subtab="listen" hidden={sub.ai !== 'listen'}><div class="cards-2"><AiPanel part="listen" /></div></div>
  <div data-subtab="lyrics" hidden={sub.ai !== 'lyrics'}><div class="cards-2"><AiWorkPanel /></div></div>
{:else if id === 'users'}
  <div class="cards-2"><UsersPanel /></div>
{:else if id === 'editor'}
  <LibraryEditor />
{:else if id === 'protected'}
  <div class="cards-2"><ProtectedPanel /></div>
{:else if id === 'go'}
  <div class="cards-2"><GoPanel /></div>
{:else if id === 'nodes'}
  <div class="cards-2"><NodesPanel /></div>
{:else if id === 'visuals'}
  <VisualsPanel />
{:else if id === 'rooms'}
  <div class="cards-2"><RoomsPanel /></div>
{:else if id === 'log'}
  <section class="card">
    <div class="log-head">
      <h2>{t('admin.request_log')}</h2>
      <button class="small-button" disabled={!log.length} onclick={clearLog}>{t('common.clear')}</button>
    </div>
    {#each log as entry, i (i)}
      <div class="log">
        <span class="muted small at">{time(entry.at)}</span>
        <div class="grow">
          <div class="ellipsis">{entry.title ?? t('admin.removed_song')}{entry.artist ? ` · ${entry.artist}` : ''}</div>
          <div class="muted small ellipsis">
            {entry.name ?? t('admin.unknown')} · <span class="mono">{entry.mac ?? entry.ip ?? t('admin.no_address')}</span>
          </div>
        </div>
      </div>
    {:else}
      <p class="muted">{t('admin.no_requests')}</p>
    {/each}
  </section>
{:else if id === 'rules' && settings}
  <section class="card">
    <h2>{t('admin.rules')}</h2>
    <form class="settings" onsubmit={saveSettings}>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.guest_mode} />
        {t('admin.guest_mode')}
      </label>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.tv_page} />
        <span>
          {t('admin.tv_page_before')}
          <a href="/tv" target="_blank" rel="noopener">/tv</a> {t('admin.tv_page_after')}
        </span>
      </label>
      {#if settings.tv_page}
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.tv_sound} />
          <span>
            {t('admin.tv_sound')}
          </span>
        </label>
      {/if}
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.play_here} />
        {t('admin.play_here')}
      </label>
      <label>
        {t('admin.max_queue')}
        <input type="number" min="0" max="1000" bind:value={settings.max_queue} />
      </label>
      <label>
        {t('admin.per_user')}
        <input type="number" min="1" max="50" bind:value={settings.per_user_limit} />
        <span class="small muted">{t('admin.per_user_note')}</span>
      </label>
      <label>
        {t('admin.skip_votes')}
        <input type="number" min="0" max="100" bind:value={settings.skip_votes} />
        <span class="small muted">
          {t('admin.skip_votes_note')}
        </span>
      </label>
      <label>
        {t('admin.history_window')}
        <input type="number" min="0" max="500" bind:value={settings.history_window} />
      </label>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.avoid_copies} />
        {t('admin.avoid_copies')}
      </label>
      <p class="small muted">
        {t('admin.avoid_copies_note')}
      </p>
      <button class="primary">{t('common.save')}</button>
    </form>
  </section>
{:else if id === 'system'}
  <div class="cards-2">
    <AboutPanel part="build" />
    <SystemStatus />
    <AppearancePanel />
    <HouseLanguage />
    <UpdatePanel />
    <BackupPanel />
    <ResetPanel />
    <AboutPanel part="credits" />
  </div>
{:else if id === 'serverlog'}
  <LogViewer />
{:else if id === 'signins'}
  <SignInLog />
{/if}
{/snippet}

<UnlockHost />
<div class="admin" class:one-page={layout === 'page'}>
<aside class="side">
  {#if OFFER_ONE_PAGE}
    <div class="layout" role="group" aria-label={t('admin.layout')}>
      <button class:active={layout === 'page'} aria-pressed={layout === 'page'} onclick={() => setLayout('page')}>{t('admin.one_page')}</button>
      <button class:active={layout === 'sections'} aria-pressed={layout === 'sections'} onclick={() => setLayout('sections')}>{t('admin.sections')}</button>
    </div>
  {/if}
  <label class="find">
    <Icon name="search" size={20} />
    <input type="search" placeholder={t('admin.find_setting')} aria-label={t('admin.find_setting')} bind:value={query} />
  </label>
  <select
    class="phone-sections"
    aria-label={t('admin.section')}
    value={tab}
    onchange={(e) => (location.hash = `#/admin/${e.currentTarget.value}`)}
  >
    {#each adminGroups as group (group.label)}
      <optgroup label={group.label}>
        {#each group.tabs as item (item.id)}<option value={item.id}>{item.label}</option>{/each}
      </optgroup>
    {/each}
  </select>
  {#if query.trim()}
    <ul class="results">
      {#each results.slice(0, 12) as entry (entry.tab + entry.label)}
        <li>
          <button onclick={() => find(entry)}>
            <span>{entryLabel(entry)}</span>
            <span class="small muted">{adminTabLabel(entry.tab)}{entry.key && changed.includes(entry.key) ? t('admin.changed_mark') : ''}</span>
          </button>
        </li>
      {:else}
        <li class="small muted">{t('admin.nothing_matches')}</li>
      {/each}
    </ul>
  {:else}
    <nav aria-label={t('admin.sections_nav')}>
      {#each adminGroups as group (group.label)}
        <div class="group">
          <div class="group-label">{group.label}</div>
          <div class="links">
            {#each group.tabs as item (item.id)}
              {#if layout === 'page'}
                <button class="nav-item" class:active={inView === item.id} onclick={() => jump(document.getElementById(`admin-${item.id}`))}>
                  {item.label}
                  {#if item.id === 'problems' && trouble > 0}<span class="badge">{trouble.toLocaleString(locale.code)}</span>{/if}
                  {#if changedTabs.has(item.id)}<span class="changed-dot" title={t('admin.has_changed')}></span>{/if}
                </button>
                {#if (cards[item.id]?.length ?? 0) > 1}
                  <div class="cards">
                    {#each cards[item.id] as card, i (i)}
                      <button onclick={() => jump(card.el)}>{card.title}</button>
                    {/each}
                  </div>
                {/if}
              {:else}
                <a href="#/admin/{item.id}" class:active={tab === item.id} aria-current={tab === item.id ? 'page' : undefined}>
                  <Icon name={item.icon} size={20} />
                  {item.label}
                  {#if item.id === 'problems' && trouble > 0}<span class="badge">{trouble.toLocaleString(locale.code)}</span>{/if}
                  {#if changedTabs.has(item.id)}<span class="changed-dot" title={t('admin.has_changed')}></span>{/if}
                </a>
              {/if}
            {/each}
          </div>
        </div>
      {/each}
    </nav>
    {#if changedEntries.length}
      <button class="link small" onclick={() => (showChanged = !showChanged)}>
        {t(showChanged ? 'admin.hide_changed' : 'admin.show_changed', { n: changedEntries.length })}
      </button>
      {#if showChanged}
        <ul class="results">
          {#each changedEntries as entry (entry.key)}
            <li>
              <button onclick={() => find(entry)}>
                <span>{entryLabel(entry)}</span>
                <span class="small muted">{adminTabLabel(entry.tab)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/if}
</aside>

<div class="admin-content" bind:this={content}>
{#if layout === 'sections'}
  <div class="crumbs small muted"><Icon name="home" size={16} /> {t('nav.admin')} <Icon name="chevron-right" size={16} /> {adminTabLabel(tab)}</div>
  <PageTitle title={adminTabLabel(tab)} />
{/if}
<PublicNetworkNote />

{#if layout === 'page'}
  {#each allTabs as id (id)}
    <section class="admin-section" id="admin-{id}" data-tab={id}>
      <h1 class="section-title">{adminTabLabel(id)}</h1>
      {@render sectionContent(id)}
    </section>
  {/each}
{:else}
  {@render sectionContent(tab)}
{/if}

</div>
</div>

<style>
  .subtabs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0 0 14px;
    padding: 4px;
    border: 1px solid var(--edge);
    border-radius: 14px;
    background: var(--surface);
  }

  .subtabs button {
    flex: 1;
    min-height: 40px;
    padding: 0 12px;
    background: transparent;
    color: var(--muted);
  }

  .subtabs button.active {
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
    box-shadow: var(--glow-strong);
  }

  .admin {
    display: block;
  }

  /* Wide screens: the menu stays on the left. */
  @media (min-width: 900px) {
    .admin {
      display: grid;
      grid-template-columns: 240px minmax(0, 1fr);
      gap: 24px;
      align-items: start;
    }

    .side {
      position: sticky;
      top: calc(var(--nav-height, 60px) + 12px);
      max-height: calc(100vh - var(--nav-height, 60px) - 24px);
      overflow-y: auto;
    }

    .side .links {
      flex-direction: column;
    }

    .side .links a {
      justify-content: flex-start;
    }
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 14px;
    padding: 12px;
    border: 1px solid var(--edge);
    border-radius: 18px;
    background: var(--card-bg);
    box-shadow: var(--glow);
  }

  .crumbs {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }

  .phone-sections {
    width: 100%;
    border-color: var(--edge);
  }

  /* Phones pick a section from the list; wide screens have the menu. */
  .side nav {
    display: none;
  }

  @media (min-width: 900px) {
    .phone-sections {
      display: none;
    }

    .side nav {
      display: block;
    }
  }

  .find {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
    padding: 0 0 0 12px;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .find:focus-within {
    border-color: var(--edge-strong);
    box-shadow: var(--glow-strong);
  }

  .find input,
  .find input:focus-visible {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    box-shadow: none;
  }

  .layout {
    display: flex;
    gap: 4px;
    padding: 3px;
    border-radius: 10px;
    background: var(--surface-2);
  }

  .layout button {
    flex: 1;
    min-height: 32px;
    padding: 0 8px;
    border-radius: 8px;
    background: none;
    color: var(--muted);
    font-size: 0.8125rem;
  }

  .layout button.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }

  .nav-item {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 6px;
    width: 100%;
    min-height: 34px;
    padding: 0 12px;
    border-radius: 8px;
    background: none;
    color: var(--muted);
    text-align: left;
  }

  .nav-item.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }

  .cards {
    display: flex;
    flex-direction: column;
    margin: 0 0 4px 12px;
    border-left: 1px solid var(--line);
  }

  .cards button {
    min-height: 26px;
    padding: 2px 10px;
    background: none;
    color: var(--muted);
    font-size: 0.8125rem;
    text-align: left;
    justify-content: flex-start;
  }

  .cards button:hover,
  .nav-item:hover {
    color: var(--text);
  }

  /* Phones: on one page the menu keeps just the sections. */
  @media (max-width: 899px) {
    .cards {
      display: none;
    }

    .one-page .links {
      flex-direction: row;
      flex-wrap: wrap;
    }

    .one-page .nav-item {
      width: auto;
    }
  }

  .admin-section {
    scroll-margin-top: calc(var(--nav-height, 60px) + 12px);
    margin-bottom: 28px;
  }

  .admin-section :global(section.card) {
    scroll-margin-top: calc(var(--nav-height, 60px) + 12px);
  }

  .section-title {
    margin: 8px 0 12px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
    font-size: 1.4rem;
  }

  .admin-section:first-child .section-title {
    border-top: 0;
  }

  .group {
    padding: 8px 0 6px;
    border-top: 1px solid var(--edge);
  }

  .group:first-child {
    border-top: 0;
  }

  .group-label {
    margin: 4px 10px 6px;
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.14em;
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .links a {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-height: 40px;
    padding: 0 12px;
    border: 1px solid transparent;
    border-radius: 12px;
    color: var(--text);
    text-decoration: none;
  }

  .links a :global(svg) {
    flex: none;
    color: var(--accent);
  }

  .links a:hover {
    color: var(--text);
  }

  .links a.active {
    border-color: var(--edge-strong);
    background: linear-gradient(90deg, rgb(180 140 255 / 0.5), rgb(180 140 255 / 0.28));
    box-shadow: var(--glow-strong);
    color: var(--text);
    font-weight: 600;
  }

  .links a.active :global(svg) {
    color: var(--text);
  }

  .badge {
    padding: 0 7px;
    border-radius: 999px;
    background: var(--bad);
    color: #fff;
    font-size: 0.75rem;
    font-weight: 600;
  }

  .changed-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }

  .results {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .results button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    min-height: 0;
    padding: 8px 10px;
    background: none;
    color: var(--text);
    text-align: left;
  }

  .results button:hover {
    background: var(--surface-2);
  }

  .link {
    min-height: 0;
    padding: 4px;
    background: none;
    color: var(--accent);
    text-align: left;
  }

  .admin-content :global(.found) {
    animation: found 2.5s ease-out;
    border-radius: 6px;
  }

  @keyframes found {
    0%,
    40% {
      box-shadow: 0 0 0 3px var(--accent);
    }
    100% {
      box-shadow: 0 0 0 3px transparent;
    }
  }

  .item,

  .grow {
    flex: 1;
    min-width: 0;
  }

  .path {
    overflow-wrap: anywhere;
  }

  .icon {
    flex: none;
    width: 44px;
    height: 44px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }



  .small-button {
    font-size: 0.875rem;
  }



  .switch {
    gap: 6px;
    font-size: 0.8125rem;
    color: var(--muted);
    white-space: nowrap;
  }

  .switch.big {
    font-size: 1rem;
    color: var(--text);
    white-space: normal;
  }

  .log-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .log {
    display: flex;
    gap: 10px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }

  .at {
    flex: none;
    width: 64px;
    padding-top: 2px;
    font-variant-numeric: tabular-nums;
  }

  .mono {
    font-family: ui-monospace, monospace;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  textarea {
    padding: 8px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
    resize: vertical;
  }

  .settings input[type='number'] {
    width: 100px;
  }

</style>
