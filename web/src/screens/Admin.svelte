<script lang="ts">
  import { api } from '../lib/api'
  import FolderPicker from '../lib/FolderPicker.svelte'
  import Icon from '../lib/Icon.svelte'
  import AboutPanel from '../lib/AboutPanel.svelte'
  import BackupPanel from '../lib/BackupPanel.svelte'
  import LibraryExtras from '../lib/LibraryExtras.svelte'
  import LibraryProgress from '../lib/LibraryProgress.svelte'
  import { clearListCache } from '../lib/listCache'
  import LogViewer from '../lib/LogViewer.svelte'
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
  import { adminTabRows, type AdminTab } from '../lib/adminTabs'
  import Sheet from '../lib/Sheet.svelte'
  import UsersPanel from '../lib/UsersPanel.svelte'
  import GoPanel from '../lib/GoPanel.svelte'
  import { app, logout, toast } from '../lib/state.svelte'
  import type { LibraryRoot, RequestLogEntry, Settings } from '../lib/types'

  let { tab = 'library' }: { tab?: AdminTab } = $props()

  let roots = $state<LibraryRoot[]>([])
  let trackCount = $state(0)
  let artDir = $state('')
  let log = $state<RequestLogEntry[]>([])
  let settings = $state<Settings | null>(null)
  let picking = $state(false)

  $effect(() => {
    load(tab).catch((e) => toast(e.message, true))
  })

  async function load(which: AdminTab) {
    if (which === 'library') {
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
    if (settings && await attempt(() => api.saveSettings(settings!), 'Saved')) clearListCache()
  }

  const scanning = $derived(app.scan !== null && !app.scan.done)
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

<h1>Admin</h1>
<PublicNetworkNote />
<nav class="tabs" aria-label="Admin sections">
  {#each adminTabRows as row, i (i)}
    <div class="tab-row">
      {#each row as t (t.id)}
        <a href="#/admin/{t.id}" class:active={tab === t.id} aria-current={tab === t.id ? 'page' : undefined}>{t.label}</a>
      {/each}
    </div>
  {/each}
</nav>

{#if tab === 'library'}
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

  {#if settings}
    <section class="card">
      <h2>Artist library</h2>
      <form class="settings" onsubmit={saveSettings}>
        <label class="switch big">
          <input type="checkbox" bind:checked={settings.show_collaboration_artists} />
          Show all credited artists in the Artists library
        </label>
        <p class="small muted">When off, secondary "&amp;" and "feat." artists stay searchable and linked from songs, but only artists with their own music appear in Artists.</p>
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
{:else if tab === 'audio'}
  <CrossfadePanel />
  <OutputsPanel />
  <CastPanel />
  <ReplayGainPanel />
  <EqualizerPanel />
{:else if tab === 'smart'}
  <SmartPlayPanel />
{:else if tab === 'ai'}
  <AiPanel />
{:else if tab === 'users'}
  <UsersPanel />
{:else if tab === 'go'}
  <GoPanel />
{:else if tab === 'log'}
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
{:else if tab === 'rules' && settings}
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
      <button class="primary">Save</button>
    </form>
  </section>
{:else if tab === 'system'}
  <SystemStatus />
  <UpdatePanel />
  <BackupPanel />
  <LogViewer />
  <ResetPanel />
{:else if tab === 'about'}
  <AboutPanel />
{/if}

<a class="button account-link" href="#/account">Your account (password, two-step sign-in)</a>
<button class="signout" onclick={logout}>Sign out</button>

<style>
  .tabs {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 14px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface);
  }

  .tab-row {
    display: flex;
    gap: 4px;
  }

  .tabs a {
    flex: 1;
    display: grid;
    place-items: center;
    min-height: 40px;
    border-radius: 8px;
    color: var(--muted);
    text-decoration: none;
  }

  .tabs a.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
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
