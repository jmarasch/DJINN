<script lang="ts">
  import { api } from '../lib/api'
  import FolderPicker from '../lib/FolderPicker.svelte'
  import Icon from '../lib/Icon.svelte'
  import Sheet from '../lib/Sheet.svelte'
  import { app, logout, toast } from '../lib/state.svelte'
  import type { GuestSummary, LibraryRoot, RequestLogEntry, Role, Settings, User } from '../lib/types'

  type Tab = 'library' | 'users' | 'log' | 'rules'
  let { tab = 'library' }: { tab?: Tab } = $props()
  const tabs: { id: Tab; label: string }[] = [
    { id: 'library', label: 'Library' },
    { id: 'users', label: 'Users' },
    { id: 'log', label: 'Log' },
    { id: 'rules', label: 'Rules' },
  ]

  let roots = $state<LibraryRoot[]>([])
  let trackCount = $state(0)
  let artDir = $state('')
  let users = $state<User[]>([])
  let guests = $state<GuestSummary[]>([])
  let log = $state<RequestLogEntry[]>([])
  let settings = $state<Settings | null>(null)
  let picking = $state(false)
  let newUser = $state({ username: '', password: '', role: 'user' as Role })

  $effect(() => {
    load(tab).catch((e) => toast(e.message, true))
  })

  async function load(which: Tab) {
    if (which === 'library') {
      const [r, status, s] = await Promise.all([api.libraryRoots(), api.libraryStatus(), api.settings()])
      roots = r
      trackCount = status.tracks
      artDir = status.art_dir
      settings = s
      if (status.last_scan) app.scan = status.last_scan
    } else if (which === 'users') {
      ;[users, guests] = await Promise.all([api.users(), api.guests()])
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

  async function updateUser(user: User, changes: { unlimited?: boolean; role?: Role }) {
    if (await attempt(() => api.updateUser(user.id, changes))) {
      users = users.map((u) => (u.id === user.id ? { ...u, ...changes } : u))
    }
  }

  async function resetPassword(user: User) {
    const password = prompt(`New password for ${user.username} (at least 6 characters):`)
    if (password) await attempt(() => api.updateUser(user.id, { password }), `Password changed for ${user.username}`)
  }

  async function removeUser(user: User) {
    if (!confirm(`Delete ${user.username}? Their queued songs are removed too.`)) return
    if (await attempt(() => api.deleteUser(user.id))) users = users.filter((u) => u.id !== user.id)
  }

  async function addUser(e: SubmitEvent) {
    e.preventDefault()
    const ok = await attempt(
      () => api.createUser(newUser.username, newUser.password, newUser.role),
      `Created ${newUser.username}`,
    )
    if (ok) {
      newUser = { username: '', password: '', role: 'user' }
      users = await api.users()
    }
  }

  async function setGuestUnlimited(guest: GuestSummary, unlimited: boolean) {
    if (await attempt(() => api.updateUser(guest.id, { unlimited }))) {
      guests = guests.map((g) => (g.id === guest.id ? { ...g, unlimited } : g))
    }
  }

  async function removeGuest(guest: GuestSummary) {
    if (!confirm(`Remove ${guest.name}? Their queued songs are removed too.`)) return
    if (await attempt(() => api.deleteUser(guest.id))) guests = guests.filter((g) => g.id !== guest.id)
  }

  async function lookUp(force: boolean) {
    try {
      const { status } = await api.lookUpArtistInfo(force)
      toast(
        status === 'started'
          ? force
            ? 'Looking up every artist again'
            : 'Looking up new artists'
          : status === 'queued'
            ? "A lookup is running. Every artist gets rechecked right after it."
            : 'Already looking up artists. Results show up when it finishes.',
      )
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the lookup', true)
    }
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
    if (settings) await attempt(() => api.saveSettings(settings!), 'Saved')
  }

  const scanning = $derived(app.scan !== null && !app.scan.done)
  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

<h1>Admin</h1>
<nav class="tabs" aria-label="Admin sections">
  {#each tabs as t (t.id)}
    <a href="#/admin/{t.id}" class:active={tab === t.id} aria-current={tab === t.id ? 'page' : undefined}>{t.label}</a>
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
    {#if scanning && app.scan}
      <p class="small">Scanning: {app.scan.read} of {app.scan.to_read} files read</p>
      <progress max={app.scan.to_read || 1} value={app.scan.read}></progress>
    {:else if app.scan}
      <p class="small muted">Last check found {app.scan.found} files, read {app.scan.read}, removed {app.scan.removed}.</p>
    {/if}
    <button disabled={scanning} onclick={checkForChanges}>Check for changes</button>
  </section>

  <section class="card">
    <h2>Artist photos and bios</h2>
    {#if settings?.online_artist_info}
      <p class="small muted">
        Looked up on Deezer and Wikipedia after each library check{settings.musicbrainz_contact
          ? ', plus MusicBrainz for exact matches, album covers, and genres'
          : ''}. Pictures in your folders always win.
      </p>
      {#if !settings.musicbrainz_contact}
        <p class="small muted">Add a MusicBrainz contact under <a href="#/admin/rules">Rules</a> for album covers, genres, and better bio matches.</p>
      {/if}
      {#if app.lookup.running}
        <p class="small"><span class="spinner" aria-hidden="true"></span> Looking up artists... New pictures show up when it's done.</p>
      {:else if app.lookup.last && app.lookup.finished_at}
        <p class="small muted">
          Last lookup {when(app.lookup.finished_at)}: {app.lookup.last.checked} artists checked,
          {app.lookup.last.photos} photos, {app.lookup.last.bios} bios{settings.musicbrainz_contact
            ? `, ${app.lookup.last.covers} album covers, ${app.lookup.last.genres} with genres`
            : ''} found.
        </p>
      {/if}
      <div class="row-buttons">
        <button disabled={app.lookup.running} onclick={() => lookUp(false)}>Look up new artists</button>
        <button onclick={() => lookUp(true)}>Recheck all</button>
      </div>
    {:else}
      <p class="small muted">Off. Turn on "Look up artist photos and bios" under <a href="#/admin/rules">Rules</a>.</p>
    {/if}
  </section>

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
{:else if tab === 'users'}
  <section class="card">
    <h2>Accounts</h2>
    {#each users as user (user.id)}
      <div class="user">
        <div class="grow">
          <div>{user.username} {#if user.id === app.me?.id}<span class="muted small">(you)</span>{/if}</div>
          <div class="user-controls">
            <select
              value={user.role}
              disabled={user.id === app.me?.id}
              aria-label="Role for {user.username}"
              onchange={(e) => updateUser(user, { role: e.currentTarget.value as Role })}
            >
              <option value="user">User</option>
              <option value="admin">Admin</option>
            </select>
            {#if user.role !== 'admin'}
              <label class="switch">
                <input type="checkbox" checked={user.unlimited} onchange={(e) => updateUser(user, { unlimited: e.currentTarget.checked })} />
                No limit
              </label>
            {/if}
            <button class="small-button" onclick={() => resetPassword(user)}>Reset password</button>
          </div>
        </div>
        {#if user.id !== app.me?.id}
          <button class="icon" aria-label="Delete {user.username}" onclick={() => removeUser(user)}><Icon name="close" /></button>
        {/if}
      </div>
    {/each}
    <form class="new-user" onsubmit={addUser}>
      <input placeholder="Username" bind:value={newUser.username} autocapitalize="none" required />
      <input placeholder="Password" type="password" bind:value={newUser.password} minlength="6" required />
      <select bind:value={newUser.role}>
        <option value="user">User</option>
        <option value="admin">Admin</option>
      </select>
      <button class="primary">Add user</button>
    </form>
  </section>

  <section class="card">
    <h2>Guests</h2>
    <p class="small muted">People who joined with just a name, by device. A device is recognized by its MAC address on your network.</p>
    {#each guests as guest (guest.id)}
      <div class="user">
        <div class="grow">
          <div>{guest.name} <span class="muted small">{guest.requests} {guest.requests === 1 ? 'request' : 'requests'}</span></div>
          <div class="muted small mono">{guest.mac ?? `no MAC (${guest.last_ip ?? 'unknown'})`}</div>
          {#if guest.names !== guest.name}<div class="muted small">Also went by: {guest.names}</div>{/if}
          <div class="muted small">Last seen {when(guest.last_seen)}</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={guest.unlimited} onchange={(e) => setGuestUnlimited(guest, e.currentTarget.checked)} />
          No limit
        </label>
        <button class="icon" aria-label="Remove {guest.name}" onclick={() => removeGuest(guest)}><Icon name="close" /></button>
      </div>
    {:else}
      <p class="muted">No guests yet. Turn on guest mode under <a href="#/admin/rules">Rules</a>.</p>
    {/each}
  </section>
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
{:else if settings}
  <section class="card">
    <h2>Rules</h2>
    <form class="settings" onsubmit={saveSettings}>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.guest_mode} />
        Guest mode: people join with just a name, no account
      </label>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.online_artist_info} />
        Look up artist photos and bios (sends artist names to Deezer and Wikipedia)
      </label>
      <label>
        MusicBrainz contact (email or website)
        <input
          type="text"
          bind:value={settings.musicbrainz_contact}
          placeholder="Leave empty to skip MusicBrainz"
          maxlength="200"
          autocapitalize="none"
        />
        <span class="small muted">
          MusicBrainz needs no account, but asks apps to say who to contact if something goes wrong. It's sent with
          each MusicBrainz request and adds album covers, genres, and exact artist matches.
        </span>
      </label>
      <label>
        Artist names that are never split (one per line)
        <textarea rows="6" bind:value={settings.artist_keep_whole} spellcheck="false"></textarea>
        <span class="small muted">
          Songs credited to "A &amp; B" or "A feat. B" count for each artist. Band names that contain
          &amp;, commas, or "x" go here so they stay one artist. Saving re-reads the library.
        </span>
      </label>
      <label>
        Most songs in the queue at once (0 for no limit)
        <input type="number" min="0" max="1000" bind:value={settings.max_queue} />
      </label>
      <label>
        Songs each person can have queued
        <input type="number" min="1" max="50" bind:value={settings.per_user_limit} />
      </label>
      <label>
        Songs before a shuffled playlist song can repeat
        <input type="number" min="0" max="500" bind:value={settings.history_window} />
      </label>
      <button class="primary">Save</button>
    </form>
  </section>
{/if}

<button class="signout" onclick={logout}>Sign out</button>

<style>
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 14px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface);
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
  .user {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

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

  .user-controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }

  .user-controls select {
    min-height: 36px;
  }

  .small-button {
    min-height: 36px;
    padding: 0 12px;
    font-size: 0.875rem;
  }

  .row-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .new-user {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-top: 6px;
  }

  .new-user input {
    min-width: 0;
  }

  .switch {
    flex-direction: row;
    align-items: center;
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

  .switch input {
    width: 20px;
    min-height: 20px;
    accent-color: var(--accent);
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

  progress {
    width: 100%;
  }

  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    margin-right: 6px;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 50%;
    vertical-align: -1px;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .signout {
    width: 100%;
    margin: 8px 0 24px;
  }
</style>
