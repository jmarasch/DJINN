<script lang="ts">
  // Admin > Users: accounts waiting for approval, accounts, roles, how
  // people sign up, invite links, and guests.
  import { onMount } from 'svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import Sheet from './Sheet.svelte'
  import { app, toast } from './state.svelte'
  import type {
    AccountRole,
    Device,
    GuestSummary,
    Invite,
    Permissions,
    SignupSettings,
    User,
  } from './types'

  let users = $state<User[]>([])
  let guests = $state<GuestSummary[]>([])
  let roles = $state<AccountRole[]>([])
  let invites = $state<Invite[]>([])
  let signup = $state<SignupSettings | null>(null)
  let newUser = $state({ username: '', password: '', access: 'role:1' })
  let newInvite = $state({ role_id: 1, note: '', uses: '1', days: '7' })
  /** The link for the invite just made; shown once. */
  let inviteLink = $state<string | null>(null)
  /** Role being edited (id 0 for a new one). */
  let editing = $state<({ id: number; name: string } & Permissions) | null>(null)
  /** Whose signed-in devices are showing. */
  let devicesFor = $state<User | null>(null)
  let devices = $state<Device[]>([])

  const pending = $derived(users.filter((u) => u.status === 'pending'))
  const accounts = $derived(users.filter((u) => u.status !== 'pending'))

  onMount(() => {
    load().catch((e) => toast(e.message, true))
  })

  async function load() {
    ;[users, guests, roles, invites, signup] = await Promise.all([
      api.users(),
      api.guests(),
      api.roles(),
      api.invites(),
      api.signupSettings(),
    ])
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

  /** 'admin' or 'role:<id>', for one dropdown covering both. */
  const accessOf = (u: User) => (u.role === 'admin' ? 'admin' : `role:${u.role_id}`)

  async function setAccess(user: User, access: string) {
    const changes =
      access === 'admin'
        ? { role: 'admin' as const }
        : { role: 'user' as const, role_id: Number(access.slice(5)) }
    if (await attempt(() => api.updateUser(user.id, changes))) {
      users = users.map((u) => (u.id === user.id ? { ...u, ...changes } : u))
      roles = await api.roles()
    }
  }

  async function setStatus(user: User, status: 'active' | 'disabled', done: string) {
    if (await attempt(() => api.updateUser(user.id, { status }), done)) {
      users = users.map((u) => (u.id === user.id ? { ...u, status } : u))
    }
  }

  async function approve(user: User) {
    await setStatus(user, 'active', `${user.username} can sign in now`)
  }

  async function deny(user: User) {
    if (!confirm(`Turn down ${user.username}? Their account is deleted.`)) return
    if (await attempt(() => api.deleteUser(user.id))) users = users.filter((u) => u.id !== user.id)
  }

  async function setUnlimited(user: User, unlimited: boolean) {
    if (await attempt(() => api.updateUser(user.id, { unlimited }))) {
      users = users.map((u) => (u.id === user.id ? { ...u, unlimited } : u))
    }
  }

  async function resetPassword(user: User) {
    const password = prompt(`New password for ${user.username} (at least 8 characters):`)
    if (password) await attempt(() => api.updateUser(user.id, { password }), `Password changed for ${user.username}`)
  }

  async function removeUser(user: User) {
    if (!confirm(`Delete ${user.username}? Their queued songs are removed too.`)) return
    if (await attempt(() => api.deleteUser(user.id))) users = users.filter((u) => u.id !== user.id)
  }

  async function addUser(e: SubmitEvent) {
    e.preventDefault()
    const admin = newUser.access === 'admin'
    const roleId = admin ? 1 : Number(newUser.access.slice(5))
    const ok = await attempt(
      () => api.createUser(newUser.username, newUser.password, admin ? 'admin' : 'user', roleId),
      `Created ${newUser.username}`,
    )
    if (ok) {
      newUser = { username: '', password: '', access: newUser.access }
      ;[users, roles] = await Promise.all([api.users(), api.roles()])
    }
  }

  async function showDevices(user: User) {
    devices = []
    devicesFor = user
    await attempt(async () => (devices = await api.userDevices(user.id)))
  }

  async function signOutDevice(device: Device) {
    if (!devicesFor) return
    const user = devicesFor
    if (await attempt(() => api.signOutUserDevice(user.id, device.id))) {
      devices = devices.filter((d) => d.id !== device.id)
    }
  }

  async function signOutEverywhere() {
    if (!devicesFor) return
    const user = devicesFor
    if (await attempt(() => api.signOutUserEverywhere(user.id), `Signed ${user.username} out everywhere`)) {
      devices = []
    }
  }

  async function saveSignup() {
    if (signup) await attempt(() => api.saveSignupSettings(signup!), 'Saved')
  }

  async function makeInvite(e: SubmitEvent) {
    e.preventDefault()
    const made = await attempt(async () => {
      const invite = await api.createInvite({
        role_id: newInvite.role_id,
        note: newInvite.note,
        uses: newInvite.uses === 'any' ? null : Number(newInvite.uses),
        days: newInvite.days === 'never' ? null : Number(newInvite.days),
      })
      inviteLink = `${location.origin}/signup?invite=${invite.token}`
      invites = [invite, ...invites]
    })
    if (made) newInvite.note = ''
  }

  async function copyLink() {
    if (!inviteLink) return
    try {
      await navigator.clipboard.writeText(inviteLink)
      toast('Invite link copied')
    } catch {
      toast('Select the link and copy it', true)
    }
  }

  async function removeInvite(invite: Invite) {
    if (!confirm('Cancel this invite? The link stops working.')) return
    if (await attempt(() => api.deleteInvite(invite.id))) invites = invites.filter((i) => i.id !== invite.id)
  }

  function editRole(role?: AccountRole) {
    editing = role
      ? { ...role }
      : {
          id: 0,
          name: '',
          go: true,
          go_remote: false,
          max_streams: 2,
          downloads: false,
          share_playlists: true,
          submit_music: false,
          submit_without_review: false,
          unlimited_requests: false,
        }
  }

  async function saveRole(e: SubmitEvent) {
    e.preventDefault()
    if (!editing) return
    const { id, ...role } = editing
    const ok = await attempt(
      () => (id ? api.updateRole(id, role) : api.createRole(role)),
      id ? 'Role saved' : `Added ${role.name}`,
    )
    if (ok) {
      editing = null
      roles = await api.roles()
    }
  }

  async function removeRole(role: AccountRole) {
    const moved = role.accounts ? ` Its ${role.accounts} ${role.accounts === 1 ? 'account moves' : 'accounts move'} to Listener.` : ''
    if (!confirm(`Delete the ${role.name} role?${moved}`)) return
    if (await attempt(() => api.deleteRole(role.id), `Deleted ${role.name}`)) await load()
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

  const roleName = (id: number) => roles.find((r) => r.id === id)?.name ?? 'Listener'
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
  const day = (ms: number) => new Date(ms).toLocaleDateString([], { month: 'short', day: 'numeric' })

  /** "Firefox on Windows" from a user agent, or the raw text. */
  function deviceName(agent: string | null): string {
    if (!agent) return 'Unknown device'
    const browser = /Edg\//.test(agent)
      ? 'Edge'
      : /Firefox\//.test(agent)
        ? 'Firefox'
        : /Chrome\//.test(agent)
          ? 'Chrome'
          : /Safari\//.test(agent)
            ? 'Safari'
            : null
    const system = /iPhone|iPad/.test(agent)
      ? 'iPhone or iPad'
      : /Android/.test(agent)
        ? 'Android'
        : /Windows/.test(agent)
          ? 'Windows'
          : /Mac OS X/.test(agent)
            ? 'Mac'
            : /Linux/.test(agent)
              ? 'Linux'
              : null
    if (browser && system) return `${browser} on ${system}`
    return browser ?? system ?? agent.slice(0, 60)
  }

  const permissionLabels: [keyof Permissions, string, string][] = [
    ['go', 'Use DJINN GO', 'Personal listening, apart from the party.'],
    ['go_remote', 'Use GO away from home', 'Otherwise GO only works on your home network.'],
    ['downloads', 'Download songs', 'Save song files to their device.'],
    ['share_playlists', 'Share playlists', 'Let other people see and play their playlists.'],
    ['submit_music', 'Submit music', 'Upload songs for the library (coming in 0.4.1).'],
    ['submit_without_review', 'Submitted music goes straight in', 'Otherwise you review each upload.'],
    ['unlimited_requests', 'No party request limit', 'Skip the per-person limit and the queue cap.'],
  ]
  const dependsOn: Partial<Record<keyof Permissions, keyof Permissions>> = {
    go_remote: 'go',
    submit_without_review: 'submit_music',
  }

  function summary(role: AccountRole): string {
    const can = permissionLabels.filter(([key]) => role[key]).map(([, label]) => label.toLowerCase())
    return can.length ? `Can ${can.join(', ')}` : 'Party requests only'
  }
</script>

{#if pending.length}
  <section class="card">
    <h2>Waiting for approval</h2>
    {#each pending as user (user.id)}
      <div class="user">
        <div class="grow">
          <div>{user.username}</div>
          <div class="muted small">Signed up {when(user.created_at)}</div>
          <div class="user-controls">
            <select
              value={accessOf(user)}
              aria-label="Role for {user.username}"
              onchange={(e) => setAccess(user, e.currentTarget.value)}
            >
              {#each roles as role (role.id)}<option value="role:{role.id}">{role.name}</option>{/each}
            </select>
            <button class="primary small-button" onclick={() => approve(user)}>Approve</button>
            <button class="small-button" onclick={() => deny(user)}>Turn down</button>
          </div>
        </div>
      </div>
    {/each}
  </section>
{/if}

<section class="card">
  <h2>Accounts</h2>
  {#each accounts as user (user.id)}
    <div class="user" class:off={user.status === 'disabled'}>
      <div class="grow">
        <div>
          {user.username}
          {#if user.id === app.me?.id}<span class="muted small">(you)</span>{/if}
          {#if user.status === 'disabled'}<span class="badge">Turned off</span>{/if}
        </div>
        <div class="user-controls">
          <select
            value={accessOf(user)}
            disabled={user.id === app.me?.id}
            aria-label="Role for {user.username}"
            onchange={(e) => setAccess(user, e.currentTarget.value)}
          >
            <option value="admin">Admin</option>
            {#each roles as role (role.id)}<option value="role:{role.id}">{role.name}</option>{/each}
          </select>
          {#if user.role !== 'admin'}
            <label class="switch">
              <input type="checkbox" checked={user.unlimited} onchange={(e) => setUnlimited(user, e.currentTarget.checked)} />
              No limit
            </label>
          {/if}
          <button class="small-button" onclick={() => showDevices(user)}>Devices</button>
          <button class="small-button" onclick={() => resetPassword(user)}>Reset password</button>
          {#if user.id !== app.me?.id}
            {#if user.status === 'disabled'}
              <button class="small-button" onclick={() => setStatus(user, 'active', `${user.username} can sign in again`)}>Turn on</button>
            {:else}
              <button class="small-button" onclick={() => setStatus(user, 'disabled', `${user.username} is signed out and can't sign in`)}>Turn off</button>
            {/if}
          {/if}
        </div>
      </div>
      {#if user.id !== app.me?.id}
        <button class="icon" aria-label="Delete {user.username}" onclick={() => removeUser(user)}><Icon name="close" /></button>
      {/if}
    </div>
  {/each}
  <form class="new-user" onsubmit={addUser}>
    <input placeholder="Username" bind:value={newUser.username} autocapitalize="none" required />
    <input placeholder="Password" type="password" bind:value={newUser.password} minlength="8" required />
    <select bind:value={newUser.access} aria-label="Role for the new account">
      {#each roles as role (role.id)}<option value="role:{role.id}">{role.name}</option>{/each}
      <option value="admin">Admin</option>
    </select>
    <button class="primary">Add account</button>
  </form>
</section>

<section class="card">
  <h2>Roles</h2>
  <p class="small muted">What each kind of account can do. Admins can do everything.</p>
  {#each roles as role (role.id)}
    <div class="user">
      <div class="grow">
        <div>
          {role.name}
          <span class="muted small">{role.accounts} {role.accounts === 1 ? 'account' : 'accounts'}</span>
        </div>
        <div class="muted small">{summary(role)}</div>
      </div>
      <button class="small-button" onclick={() => editRole(role)}>Edit</button>
      {#if !role.builtin}
        <button class="icon" aria-label="Delete {role.name}" onclick={() => removeRole(role)}><Icon name="close" /></button>
      {/if}
    </div>
  {/each}
  <button class="small-button add" onclick={() => editRole()}>New role</button>
</section>

{#if signup}
  <section class="card">
    <h2>Sign-ups</h2>
    <label>
      Who can create an account
      <select bind:value={signup.mode} onchange={saveSignup}>
        <option value="closed">Only people you invite or add</option>
        <option value="review">Anyone, after you approve them</option>
        <option value="open">Anyone, straight away</option>
      </select>
    </label>
    {#if signup.mode !== 'closed'}
      <label>
        New accounts get the role
        <select bind:value={signup.role_id} onchange={saveSignup}>
          {#each roles as role (role.id)}<option value={role.id}>{role.name}</option>{/each}
        </select>
      </label>
      {#if signup.mode === 'open'}
        <p class="small muted">Anyone who can open DJINN can make an account. Pick a role that can't do much, or approve sign-ups instead.</p>
      {/if}
    {/if}
  </section>
{/if}

<section class="card">
  <h2>Invite links</h2>
  <p class="small muted">Send someone a link to make their own account, in the role you choose. Works even when sign-ups are closed.</p>
  <form class="invite-form" onsubmit={makeInvite}>
    <label>
      Role
      <select bind:value={newInvite.role_id}>
        {#each roles as role (role.id)}<option value={role.id}>{role.name}</option>{/each}
      </select>
    </label>
    <label>
      Can be used
      <select bind:value={newInvite.uses}>
        <option value="1">Once</option>
        <option value="5">5 times</option>
        <option value="25">25 times</option>
        <option value="any">Any number of times</option>
      </select>
    </label>
    <label>
      Works for
      <select bind:value={newInvite.days}>
        <option value="1">1 day</option>
        <option value="7">7 days</option>
        <option value="30">30 days</option>
        <option value="never">Until you cancel it</option>
      </select>
    </label>
    <label>
      Note (for you)
      <input bind:value={newInvite.note} maxlength="100" placeholder="Who it's for" />
    </label>
    <button class="primary">Make invite link</button>
  </form>
  {#if inviteLink}
    <div class="new-link">
      <p class="small">Copy this link now; it isn't shown again.</p>
      <input readonly value={inviteLink} onfocus={(e) => e.currentTarget.select()} aria-label="Invite link" />
      <button class="small-button" onclick={copyLink}>Copy link</button>
    </div>
  {/if}
  {#each invites as invite (invite.id)}
    <div class="user">
      <div class="grow">
        <div>{invite.note || roleName(invite.role_id)} <span class="muted small">{invite.role_name}</span></div>
        <div class="muted small">
          {invite.uses_left === null ? 'Any number of uses' : `${invite.uses_left} ${invite.uses_left === 1 ? 'use' : 'uses'} left`},
          {invite.expires_at === null ? 'no end date' : `until ${day(invite.expires_at)}`}
          {#if invite.used}, used {invite.used} {invite.used === 1 ? 'time' : 'times'}{/if}
        </div>
      </div>
      <button class="icon" aria-label="Cancel invite" onclick={() => removeInvite(invite)}><Icon name="close" /></button>
    </div>
  {/each}
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

{#if editing}
  <Sheet title={editing.id ? `Edit ${editing.name}` : 'New role'} onclose={() => (editing = null)}>
    <form class="role-form" onsubmit={saveRole}>
      <label>
        Name
        <input bind:value={editing.name} maxlength="32" required />
      </label>
      {#each permissionLabels as [key, label, hint] (key)}
        {@const needs = dependsOn[key]}
        <label class="switch big" class:indent={needs}>
          <input type="checkbox" bind:checked={editing[key] as boolean} disabled={needs ? !editing[needs] : false} />
          <span>
            {label}
            <span class="small muted block">{hint}</span>
          </span>
        </label>
      {/each}
      <label>
        GO streams at once
        <select bind:value={editing.max_streams} disabled={!editing.go}>
          <option value={1}>1</option>
          <option value={2}>2</option>
          <option value={3}>3</option>
          <option value={5}>5</option>
          <option value={0}>No limit</option>
        </select>
      </label>
      <button class="primary">{editing.id ? 'Save' : 'Add role'}</button>
    </form>
  </Sheet>
{/if}

{#if devicesFor}
  <Sheet title="{devicesFor.username}'s devices" onclose={() => (devicesFor = null)}>
    {#each devices as device (device.id)}
      <div class="user">
        <div class="grow">
          <div>{deviceName(device.user_agent)}</div>
          <div class="muted small">
            {device.ip ?? 'Unknown address'}, last used {when(device.last_seen_at)}, signed in {day(device.created_at)}
          </div>
        </div>
        <button class="small-button" onclick={() => signOutDevice(device)}>Sign out</button>
      </div>
    {:else}
      <p class="muted">Not signed in anywhere.</p>
    {/each}
    {#if devices.length > 1}
      <button class="signout-all" onclick={signOutEverywhere}>Sign out everywhere</button>
    {/if}
  </Sheet>
{/if}

<style>
  .user {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .user.off > .grow > div:first-child {
    color: var(--muted);
  }

  .grow {
    flex: 1;
    min-width: 0;
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

  .badge {
    margin-left: 6px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 0.75rem;
  }

  .new-user,
  .invite-form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-top: 6px;
  }

  .new-user input,
  .invite-form input {
    min-width: 0;
  }

  .new-link {
    display: grid;
    gap: 6px;
    padding: 10px;
    border-radius: 10px;
    background: var(--surface-2);
  }

  .new-link p {
    margin: 0;
  }

  .add {
    align-self: flex-start;
  }

  .switch {
    gap: 6px;
    font-size: 0.8125rem;
    color: var(--muted);
    white-space: nowrap;
  }

  .switch.big {
    align-items: flex-start;
    font-size: 1rem;
    color: var(--text);
    white-space: normal;
  }

  .switch.big input {
    margin-top: 2px;
  }

  .indent {
    margin-left: 28px;
  }

  .block {
    display: block;
  }

  .role-form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .mono {
    font-family: ui-monospace, monospace;
  }

  .signout-all {
    width: 100%;
    margin-top: 12px;
  }
</style>
