<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import { app, logout, toast } from '../lib/state.svelte'
  import type { GuestSummary, PoolEntry, RequestLogEntry, Role, Settings, User } from '../lib/types'

  let pool = $state<PoolEntry[]>([])
  let users = $state<User[]>([])
  let guests = $state<GuestSummary[]>([])
  let log = $state<RequestLogEntry[]>([])
  let settings = $state<Settings | null>(null)
  let trackCount = $state(0)
  let volume = $state(app.player.volume)
  let newUser = $state({ username: '', password: '', role: 'user' as Role })

  async function load() {
    ;[pool, users, settings, guests, log] = await Promise.all([
      api.pool(),
      api.users(),
      api.settings(),
      api.guests(),
      api.requestLog(50),
    ])
    const status = await api.libraryStatus()
    trackCount = status.tracks
    if (status.last_scan) app.scan = status.last_scan
  }

  onMount(() => {
    load().catch((e) => toast(e.message, true))
  })

  // Follow volume changes made elsewhere.
  $effect(() => {
    volume = app.player.volume
  })

  let volumeTimer: ReturnType<typeof setTimeout>
  function onVolume() {
    clearTimeout(volumeTimer)
    volumeTimer = setTimeout(() => attempt(() => api.setVolume(volume)), 120)
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

  async function removePool(entry: PoolEntry) {
    if (await attempt(() => api.removeFromPool(entry.id))) {
      pool = pool.filter((p) => p.id !== entry.id)
    }
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

  async function removeUser(user: User) {
    if (!confirm(`Delete ${user.username}? Their queued songs are removed too.`)) return
    if (await attempt(() => api.deleteUser(user.id))) {
      users = users.filter((u) => u.id !== user.id)
    }
  }

  async function setUnlimited(id: number, unlimited: boolean) {
    if (await attempt(() => api.setUnlimited(id, unlimited))) {
      users = users.map((u) => (u.id === id ? { ...u, unlimited } : u))
      guests = guests.map((g) => (g.id === id ? { ...g, unlimited } : g))
    }
  }

  async function removeGuest(guest: GuestSummary) {
    if (!confirm(`Remove ${guest.name}? Their queued songs are removed too.`)) return
    if (await attempt(() => api.deleteUser(guest.id))) {
      guests = guests.filter((g) => g.id !== guest.id)
    }
  }

  const time = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })

  async function saveSettings(e: SubmitEvent) {
    e.preventDefault()
    if (settings) await attempt(() => api.saveSettings(settings!), 'Settings saved')
  }

  const scanning = $derived(app.scan !== null && !app.scan.done)
</script>

<h1>Admin</h1>

<a class="button join-link" href="/join" target="_blank" rel="noopener">Show join QR code</a>

<section class="card">
  <h2>Player</h2>
  <div class="row">
    {#if app.player.state === 'paused'}
      <button class="round" aria-label="Play" onclick={() => attempt(api.play)}><Icon name="play" /></button>
    {:else}
      <button class="round" aria-label="Pause" onclick={() => attempt(api.pause)}><Icon name="pause" /></button>
    {/if}
    <button class="round" aria-label="Skip" onclick={() => attempt(api.skip)}><Icon name="skip" /></button>
    <span class="muted">{app.player.state}</span>
  </div>
  <label class="volume">
    <span class="volume-head">Volume <span class="muted">{volume}</span></span>
    <input type="range" min="0" max="100" bind:value={volume} oninput={onVolume} />
  </label>
  <p class="small muted">
    {#if app.output.error}
      <span class="error">Output problem: {app.output.error}</span>
    {:else if app.output.device}
      Playing on {app.output.device} at {(app.output.sample_rate ?? 0) / 1000} kHz
    {:else}
      No audio output yet
    {/if}
  </p>
</section>

<section class="card">
  <h2>Auto pool</h2>
  <p class="small muted">
    Plays when nobody has requested anything. Add albums and artists from their pages.
  </p>
  {#each pool as entry (entry.id)}
    <div class="item">
      <span class="tag">{entry.kind}</span>
      <span class="grow ellipsis">{entry.name ?? '(missing)'}</span>
      <button class="icon" aria-label="Remove {entry.name}" onclick={() => removePool(entry)}>
        <Icon name="close" />
      </button>
    </div>
  {:else}
    <p class="muted">The pool is empty, so DJINN stays quiet when the queue runs out.</p>
  {/each}
</section>

<section class="card">
  <h2>Library</h2>
  <p class="muted">{trackCount} songs</p>
  {#if scanning && app.scan}
    <p class="small">Scanning: {app.scan.read} of {app.scan.to_read} files read</p>
    <progress max={app.scan.to_read || 1} value={app.scan.read}></progress>
  {:else if app.scan}
    <p class="small muted">
      Last scan found {app.scan.found} files, read {app.scan.read}, removed {app.scan.removed}.
    </p>
  {/if}
  <button disabled={scanning} onclick={() => attempt(api.rescan, 'Rescan started')}>Rescan library</button>
</section>

<section class="card">
  <h2>Users</h2>
  {#each users as user (user.id)}
    <div class="item">
      <span class="grow">{user.username}</span>
      {#if user.role !== 'admin'}
        <label class="switch">
          <input type="checkbox" checked={user.unlimited} onchange={(e) => setUnlimited(user.id, e.currentTarget.checked)} />
          No limit
        </label>
      {/if}
      <span class="tag">{user.role}</span>
      {#if user.id !== app.me?.id}
        <button class="icon" aria-label="Delete {user.username}" onclick={() => removeUser(user)}>
          <Icon name="close" />
        </button>
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
  <p class="small muted">
    People who joined with just a name, by device. A device is recognized by its MAC address on
    your network, so it keeps its identity next time.
  </p>
  {#each guests as guest (guest.id)}
    <div class="guest">
      <div class="grow">
        <div>{guest.name} <span class="muted small">{guest.requests} {guest.requests === 1 ? 'request' : 'requests'}</span></div>
        <div class="muted small mono">{guest.mac ?? `no MAC (${guest.last_ip ?? 'unknown'})`}</div>
        {#if guest.names !== guest.name}<div class="muted small">Also went by: {guest.names}</div>{/if}
        <div class="muted small">Last seen {when(guest.last_seen)}</div>
      </div>
      <label class="switch">
        <input type="checkbox" checked={guest.unlimited} onchange={(e) => setUnlimited(guest.id, e.currentTarget.checked)} />
        No limit
      </label>
      <button class="icon" aria-label="Remove {guest.name}" onclick={() => removeGuest(guest)}>
        <Icon name="close" />
      </button>
    </div>
  {:else}
    <p class="muted">No guests yet. Turn on guest mode under Rules.</p>
  {/each}
</section>

<section class="card">
  <h2>Request log</h2>
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
    <p class="muted">No requests yet.</p>
  {/each}
</section>

{#if settings}
  <section class="card">
    <h2>Rules</h2>
    <form class="settings" onsubmit={saveSettings}>
      <label class="switch big">
        <input type="checkbox" bind:checked={settings.guest_mode} />
        Guest mode: people join with just a name, no account
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
        Songs before a pool song can repeat
        <input type="number" min="0" max="500" bind:value={settings.history_window} />
      </label>
      <button>Save</button>
    </form>
  </section>
{/if}

<button class="signout" onclick={logout}>Sign out</button>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .volume {
    margin-top: 12px;
  }

  .volume input {
    width: 100%;
  }

  .volume-head {
    display: flex;
    justify-content: space-between;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .tag {
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 0.75rem;
    text-transform: uppercase;
  }

  .icon {
    width: 44px;
    height: 44px;
    color: var(--muted);
  }

  .new-user {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-top: 12px;
  }

  .new-user input {
    min-width: 0;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .settings input {
    width: 90px;
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

  .guest {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
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

  progress {
    width: 100%;
  }

  .join-link {
    width: 100%;
    margin-bottom: 16px;
  }

  .signout {
    width: 100%;
    margin: 8px 0 24px;
  }
</style>
