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
  import ResetPanel from '../lib/ResetPanel.svelte'
  import SystemStatus from '../lib/SystemStatus.svelte'
  import UpdatePanel from '../lib/UpdatePanel.svelte'
  import AiPanel from '../lib/AiPanel.svelte'
  import CastPanel from '../lib/CastPanel.svelte'
  import CrossfadePanel from '../lib/CrossfadePanel.svelte'
  import EqualizerPanel from '../lib/EqualizerPanel.svelte'
  import OutputsPanel from '../lib/OutputsPanel.svelte'
  import PublicNetworkNote from '../lib/PublicNetworkNote.svelte'
  import ReplayGainPanel from '../lib/ReplayGainPanel.svelte'
  import SmartPlayPanel from '../lib/SmartPlayPanel.svelte'
  import { adminGroups, adminTabLabel, type AdminTab } from '../lib/adminTabs'
  import { adminEntries, searchAdmin, type AdminEntry } from '../lib/adminSearch'
  import AdminHome from '../lib/AdminHome.svelte'
  import StatsPanel from '../lib/StatsPanel.svelte'
  import SearchesWishes from '../lib/SearchesWishes.svelte'
  import { tick } from 'svelte'
  import Sheet from '../lib/Sheet.svelte'
  import UsersPanel from '../lib/UsersPanel.svelte'
  import GoPanel from '../lib/GoPanel.svelte'
  import NodesPanel from '../lib/NodesPanel.svelte'
  import VisualsPanel from '../lib/VisualsPanel.svelte'
  import RoomsPanel from '../lib/RoomsPanel.svelte'
  import { app, logout, toast } from '../lib/state.svelte'
  import type { LibraryRoot, RequestLogEntry, Settings } from '../lib/types'

  let { tab = 'home' }: { tab?: AdminTab } = $props()

  // Two layouts: one section at a time, or every section on one page with
  // the menu jumping to each section and card. Only sections are offered for
  // now; set this to bring the one-page layout and its switch back.
  const OFFER_ONE_PAGE = false
  type Layout = 'sections' | 'page'
  let layout = $state<Layout>(OFFER_ONE_PAGE ? storedLayout() : 'sections')
  const allTabs: AdminTab[] = adminGroups.flatMap((g) => g.tabs.map((t) => t.id))

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
    api.libraryProblems().then((p) => (trouble = p.trouble)).catch(() => {})
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
    const want = entry.label.toLowerCase().slice(0, 40)
    for (let i = 0; i < 30; i++) {
      const found = [...document.querySelectorAll<HTMLElement>('.admin-content :is(h2, h3, label, p, button, dt, a)')]
        .filter((el) => (el.textContent ?? '').toLowerCase().includes(want))
        .sort((a, b) => (a.textContent ?? '').length - (b.textContent ?? '').length)[0]
      if (found) {
        found.scrollIntoView({ behavior: 'smooth', block: 'center' })
        found.classList.add('found')
        setTimeout(() => found.classList.remove('found'), 2500)
        return
      }
      await new Promise((r) => setTimeout(r, 100))
    }
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
      toast(e instanceof Error ? e.message : 'That did not work', true)
      return false
    }
  }

  async function addRoot(path: string) {
    picking = false
    await attempt(async () => (roots = await api.addLibraryRoot(path)), 'Folder added. Scanning it now.')
  }

  async function removeRoot(root: LibraryRoot) {
    if (!confirm(`Remove ${root.path} from the library? Its songs disappear from DJINN (the files stay).`)) return
    await attempt(async () => (roots = await api.removeLibraryRoot(root.path)), 'Folder removed')
  }

  async function checkForChanges() {
    try {
      const { status } = await api.rescan()
      toast(status === 'started' ? 'Checking for changes' : 'Already checking for changes')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the check', true)
    }
  }

  async function clearLog() {
    if (!confirm('Clear the request log? Top songs on artist pages keep counting.')) return
    if (await attempt(() => api.clearRequestLog(), 'Request log cleared')) log = []
  }

  async function saveSettings(e: SubmitEvent) {
    e.preventDefault()
    if (settings && await attempt(() => api.saveSettings(settings!), 'Saved')) {
      clearListCache()
      api.changedSettings().then((c) => (changed = c)).catch(() => {})
    }
  }

  const scanning = $derived(app.scan !== null && !app.scan.done)
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

{#snippet sectionContent(t: AdminTab)}
{#if t === 'home'}
  <AdminHome />
  <SystemStatus />
  <AboutPanel />
{:else if t === 'stats'}
  <StatsPanel />
  <SearchesWishes />
{:else if t === 'problems'}
  <ScheduledCheck />
  <LibraryProblems />
  <LibraryDuplicates />
{:else if t === 'library'}
  <section class="card">
    <h2>Music folders</h2>
    {#each roots as root (root.path)}
      <div class="item">
        <Icon name="folder" size={20} />
        <div class="grow">
          <div class="path">{root.path}</div>
          {#if !root.available}<div class="small error">Not found right now (unplugged or offline?)</div>{/if}
          {#if root.from_config}<div class="small muted">From config.toml</div>{/if}
        </div>
        {#if !root.from_config}
          <button class="icon" aria-label="Remove {root.path}" onclick={() => removeRoot(root)}><Icon name="close" /></button>
        {/if}
      </div>
    {:else}
      <p class="muted">No music folders yet. Add the folder where your music lives.</p>
    {/each}
    <button class="primary" onclick={() => (picking = true)}>Add a folder</button>
  </section>

  <section class="card">
    <h2>Library</h2>
    <p class="muted">{trackCount} songs</p>
    <LibraryProgress />
    {#if app.scan && !scanning}
      <p class="small muted">Last check found {app.scan.found} files, read {app.scan.read}, removed {app.scan.removed}.</p>
    {/if}
    <button disabled={scanning} onclick={checkForChanges}>Check for changes</button>
  </section>

  <p class="small muted">Files that didn't make it in, copies of songs and the nightly check are under <a href="#/admin/problems">Problems</a>.</p>

  {#if settings}
    <section class="card">
      <h2>Artist library</h2>
      <form class="settings" onsubmit={saveSettings}>
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.show_collaboration_artists} />
          Show all credited artists in the Artists library
        </label>
        <p class="small muted">When off, secondary "&amp;" and "feat." artists stay searchable and linked from songs, but only artists with their own music appear in Artists.</p>
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.original_years} />
          Use the original release year
        </label>
        <p class="small muted">A reissue's files often carry the reissue's date too. When on, an original release date in the tags wins. Changing this re-reads the library. Years you've corrected in the app stay as you set them.</p>
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.album_folders} />
          Keep albums with the same name apart
        </label>
        <p class="small muted">When two folders hold different songs at the same place on an album, such as two different "track 1"s, they show as separate albums, each with its folder's name. Discs of one album stay together.</p>
        <label>
          Songs and artists a genre page loads at a time
          <input type="number" min="10" max="500" bind:value={settings.genre_page_size} />
        </label>
        <label>
          Hours before a genre's order is shuffled again
          <input type="number" min="0" max="8760" bind:value={settings.genre_refresh_hours} />
          <span class="small muted">Genre pages show everything in a shuffled order, with what's been played most nearer the top. 0 shuffles on every visit.</span>
        </label>
        <label>
          A play counts once this much of the song is heard: {settings.play_count_percent}%
          <input type="range" min="0" max="100" step="5" bind:value={settings.play_count_percent} />
          <span class="small muted">For the play counts in the library lists. A song started again for a moment isn't a play. 0 counts every play.</span>
        </label>
        <label>
          Names that should never be split, one per line
          <textarea rows="6" bind:value={settings.artist_keep_whole} spellcheck="false"></textarea>
          <span class="small muted">
            Songs credited to "A &amp; B" or "A feat. B" still link to each artist. Band names that contain &amp;, commas,
            or "x" go here so they stay one artist. Saving re-reads the library.
          </span>
        </label>
        <button class="primary">Save</button>
      </form>
    </section>

    <LibraryExtras bind:settings />
  {/if}

  <section class="card">
    <h2>Image cache</h2>
    <p class="small muted">
      Covers and artist photos DJINN finds are kept here, on this computer. Your music folders are never changed.
    </p>
    <p class="path small mono">{artDir}</p>
    <button onclick={() => attempt(api.openArtFolder)}>Open image folder</button>
  </section>

  {#if picking}
    <Sheet title="Pick a music folder" onclose={() => (picking = false)}>
      <FolderPicker onpick={addRoot} />
    </Sheet>
  {/if}
{:else if t === 'audio'}
  <CrossfadePanel />
  <OutputsPanel />
  <CastPanel />
  <ReplayGainPanel />
  <EqualizerPanel />
{:else if t === 'smart'}
  <SmartPlayPanel />
{:else if t === 'ai'}
  <AiPanel />
{:else if t === 'users'}
  <UsersPanel />
{:else if t === 'go'}
  <GoPanel />
{:else if t === 'nodes'}
  <NodesPanel />
{:else if t === 'visuals'}
  <VisualsPanel />
{:else if t === 'rooms'}
  <RoomsPanel />
{:else if t === 'log'}
  <section class="card">
    <div class="log-head">
      <h2>Request log</h2>
      <button class="small-button" disabled={!log.length} onclick={clearLog}>Clear</button>
    </div>
    {#each log as entry, i (i)}
      <div class="log">
        <span class="muted small at">{time(entry.at)}</span>
        <div class="grow">
          <div class="ellipsis">{entry.title ?? '(removed song)'}{entry.artist ? ` · ${entry.artist}` : ''}</div>
          <div class="muted small ellipsis">
            {entry.name ?? 'unknown'} · <span class="mono">{entry.mac ?? entry.ip ?? 'no address'}</span>
          </div>
        </div>
      </div>
    {:else}
      <p class="muted">No requests since the log was last cleared.</p>
    {/each}
  </section>
{:else if t === 'rules' && settings}
  <section class="card">
    <h2>Rules</h2>
    <form class="settings" onsubmit={saveSettings}>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.guest_mode} />
        Guest mode: people join with just a name, no account
      </label>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.tv_page} />
        <span>
          TV page: what's playing, what's next, and the join code, for a TV or tablet. Open
          <a href="/tv" target="_blank" rel="noopener">/tv</a> on that screen; no sign-in needed.
        </span>
      </label>
      {#if settings.tv_page}
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.tv_sound} />
          <span>
            Sound on the TV page: a button on it plays the party, for a TV browser that isn't cast to.
            Anyone who can open the page can listen, signed in or not.
          </span>
        </label>
      {/if}
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.play_here} />
        Play here: a Tune In button on the Playing tab lets anyone play the party on their own phone
        or computer, a few seconds behind the speakers
      </label>
      <label>
        Most songs in the queue at once (0 for no limit)
        <input type="number" min="0" max="1000" bind:value={settings.max_queue} />
      </label>
      <label>
        Songs each person can have queued
        <input type="number" min="1" max="50" bind:value={settings.per_user_limit} />
        <span class="small muted">Asking for a song someone else queued counts too (it moves that song up a place).</span>
      </label>
      <label>
        Votes needed to skip a song (0 turns skip voting off)
        <input type="number" min="0" max="100" bind:value={settings.skip_votes} />
        <span class="small muted">
          Guests get a "Vote to skip" button on the playing screen. Whoever requested a song can always skip it
          alone, and admins skip right away.
        </span>
      </label>
      <label>
        Songs before a shuffled playlist song can repeat
        <input type="number" min="0" max="500" bind:value={settings.history_window} />
      </label>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.avoid_copies} />
        Don't play another copy of a song that just played
      </label>
      <p class="small muted">
        When your library has the same song more than once (see Possible duplicates under Library), the auto mix
        treats every copy as played.
      </p>
      <button class="primary">Save</button>
    </form>
  </section>
{:else if t === 'system'}
  <UpdatePanel />
  <BackupPanel />
  <ResetPanel />
{:else if t === 'serverlog'}
  <LogViewer />
{:else if t === 'signins'}
  <SignInLog />
{/if}
{/snippet}

<div class="admin" class:one-page={layout === 'page'}>
<aside class="side">
  {#if OFFER_ONE_PAGE}
    <div class="layout" role="group" aria-label="Layout">
      <button class:active={layout === 'page'} aria-pressed={layout === 'page'} onclick={() => setLayout('page')}>One page</button>
      <button class:active={layout === 'sections'} aria-pressed={layout === 'sections'} onclick={() => setLayout('sections')}>Sections</button>
    </div>
  {/if}
  <input class="find" type="search" placeholder="Find a setting" aria-label="Find a setting" bind:value={query} />
  {#if query.trim()}
    <ul class="results">
      {#each results.slice(0, 12) as entry (entry.tab + entry.label)}
        <li>
          <button onclick={() => find(entry)}>
            <span>{entry.label}</span>
            <span class="small muted">{adminTabLabel(entry.tab)}{entry.key && changed.includes(entry.key) ? ', changed' : ''}</span>
          </button>
        </li>
      {:else}
        <li class="small muted">Nothing matches.</li>
      {/each}
    </ul>
  {:else}
    <nav aria-label="Admin sections">
      {#each adminGroups as group (group.label)}
        <div class="group">
          <div class="group-label">{group.label}</div>
          <div class="links">
            {#each group.tabs as t (t.id)}
              {#if layout === 'page'}
                <button class="nav-item" class:active={inView === t.id} onclick={() => jump(document.getElementById(`admin-${t.id}`))}>
                  {t.label}
                  {#if t.id === 'problems' && trouble > 0}<span class="badge">{trouble.toLocaleString()}</span>{/if}
                  {#if changedTabs.has(t.id)}<span class="changed-dot" title="Has settings changed from the default"></span>{/if}
                </button>
                {#if (cards[t.id]?.length ?? 0) > 1}
                  <div class="cards">
                    {#each cards[t.id] as card, i (i)}
                      <button onclick={() => jump(card.el)}>{card.title}</button>
                    {/each}
                  </div>
                {/if}
              {:else}
                <a href="#/admin/{t.id}" class:active={tab === t.id} aria-current={tab === t.id ? 'page' : undefined}>
                  {t.label}
                  {#if t.id === 'problems' && trouble > 0}<span class="badge">{trouble.toLocaleString()}</span>{/if}
                  {#if changedTabs.has(t.id)}<span class="changed-dot" title="Has settings changed from the default"></span>{/if}
                </a>
              {/if}
            {/each}
          </div>
        </div>
      {/each}
    </nav>
    {#if changedEntries.length}
      <button class="link small" onclick={() => (showChanged = !showChanged)}>
        {showChanged ? 'Hide' : 'Show'} settings changed from the default ({changedEntries.length})
      </button>
      {#if showChanged}
        <ul class="results">
          {#each changedEntries as entry (entry.key)}
            <li>
              <button onclick={() => find(entry)}>
                <span>{entry.label}</span>
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
{#if layout === 'sections'}<h1>{adminTabLabel(tab)}</h1>{/if}
<PublicNetworkNote />

{#if layout === 'page'}
  {#each allTabs as t (t)}
    <section class="admin-section" id="admin-{t}" data-tab={t}>
      <h1 class="section-title">{t === 'home' ? 'Home' : adminTabLabel(t)}</h1>
      {@render sectionContent(t)}
    </section>
  {/each}
{:else}
  {@render sectionContent(tab)}
{/if}

<a class="button account-link" href="#/account">Your account (password, two-step sign-in)</a>
<button class="signout" onclick={logout}>Sign out</button>
</div>
</div>

<style>
  .admin {
    display: block;
  }

  /* Wide screens: the menu stays on the left. */
  @media (min-width: 900px) {
    .admin {
      display: grid;
      grid-template-columns: 220px minmax(0, 1fr);
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
    padding: 10px;
    border-radius: 12px;
    background: var(--surface);
  }

  .find {
    width: 100%;
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

  .group-label {
    margin: 4px 4px 2px;
    color: var(--muted);
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
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
    gap: 6px;
    min-height: 38px;
    padding: 0 12px;
    border-radius: 8px;
    color: var(--muted);
    text-decoration: none;
  }

  .links a.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
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

  .account-link {
    width: 100%;
    margin-top: 8px;
  }

  .signout {
    width: 100%;
    margin: 8px 0 24px;
  }
</style>
