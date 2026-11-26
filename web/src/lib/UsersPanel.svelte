<script lang="ts">
  // Admin > Users: accounts waiting for approval, accounts, roles, how
  // people sign up, invite links, and guests.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { deviceName } from './format'
  import Icon from './Icon.svelte'
  import { unlocked } from './protected.svelte'
  import Sheet from './Sheet.svelte'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
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
  let newUser = $state({ username: '', password: '', confirm: '', access: 'role:1' })
  let editingUsername = $state<User | null>(null)
  let usernameDraft = $state('')
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

  /** Accounts locked after too many wrong two-step codes. */
  let locked = $state<number[]>([])

  async function unlock(user: User) {
    if (await attempt(() => api.unlockAccount(user.id), t('up.can_sign_in_again', { name: user.username }))) {
      locked = locked.filter((id) => id !== user.id)
    }
  }

  onMount(() => {
    api.lockedAccounts().then((ids) => (locked = ids)).catch(() => {})
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

  // Changes to an admin's account (and making one) need the protected areas
  // unlocked; the server says so and the unlock box opens.
  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await unlocked(action)
      if (done) toast(done)
      return true
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
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
    await setStatus(user, 'active', t('up.can_sign_in_now', { name: user.username }))
  }

  async function deny(user: User) {
    if (!confirm(t('up.deny_q', { name: user.username }))) return
    if (await attempt(() => api.deleteUser(user.id))) users = users.filter((u) => u.id !== user.id)
  }

  async function setUnlimited(user: User, unlimited: boolean) {
    if (await attempt(() => api.updateUser(user.id, { unlimited }))) {
      users = users.map((u) => (u.id === user.id ? { ...u, unlimited } : u))
    }
  }

  async function resetPassword(user: User) {
    const password = prompt(t('up.new_password_q', { name: user.username }))
    if (password) await attempt(() => api.updateUser(user.id, { password }), t('up.password_changed', { name: user.username }))
  }

  function editUsername(user: User) {
    editingUsername = user
    usernameDraft = user.username
  }

  async function saveUsername(e: SubmitEvent) {
    e.preventDefault()
    if (!editingUsername) return
    const user = editingUsername
    const username = usernameDraft.trim()
    if (username === user.username) {
      editingUsername = null
      return
    }
    if (await attempt(() => api.updateUser(user.id, { username }), t('up.username_changed', { name: username }))) {
      users = users.map((u) => (u.id === user.id ? { ...u, username } : u))
      editingUsername = null
    }
  }

  async function resetTwoStep(user: User) {
    if (!confirm(t('up.two_step_off_q', { name: user.username }))) return
    if (await attempt(() => api.turnOffUserTwoStep(user.id), t('up.two_step_off', { name: user.username }))) {
      users = users.map((u) => (u.id === user.id ? { ...u, two_step: false } : u))
    }
  }

  async function removeUser(user: User) {
    if (!confirm(t('up.delete_q', { name: user.username }))) return
    if (await attempt(() => api.deleteUser(user.id))) users = users.filter((u) => u.id !== user.id)
  }

  async function addUser(e: SubmitEvent) {
    e.preventDefault()
    if (newUser.password !== newUser.confirm) {
      toast(t('up.no_match'), true)
      return
    }
    const admin = newUser.access === 'admin'
    const roleId = admin ? 1 : Number(newUser.access.slice(5))
    const ok = await attempt(
      () => api.createUser(newUser.username, newUser.password, admin ? 'admin' : 'user', roleId),
      t('up.created', { name: newUser.username }),
    )
    if (ok) {
      newUser = { username: '', password: '', confirm: '', access: newUser.access }
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
    if (await attempt(() => api.signOutUserEverywhere(user.id), t('up.signed_out_all', { name: user.username }))) {
      devices = []
    }
  }

  async function saveSignup() {
    if (signup) await attempt(() => api.saveSignupSettings(signup!), t('common.saved'))
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
      toast(t('up.link_copied'))
    } catch {
      toast(t('up.copy_by_hand'), true)
    }
  }

  async function removeInvite(invite: Invite) {
    if (!confirm(t('up.cancel_invite_q'))) return
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
          require_two_step: false,
          convert: true,
          auto_mix: false,
        }
  }

  async function saveRole(e: SubmitEvent) {
    e.preventDefault()
    if (!editing) return
    const { id, ...role } = editing
    const ok = await attempt(
      () => (id ? api.updateRole(id, role) : api.createRole(role)),
      id ? t('up.role_saved') : t('up.added', { name: role.name }),
    )
    if (ok) {
      editing = null
      roles = await api.roles()
    }
  }

  async function removeRole(role: AccountRole) {
    const moved = role.accounts ? t('up.role_moves', { n: role.accounts }) : ''
    if (!confirm(t('up.delete_role_q', { name: role.name, moved }))) return
    if (await attempt(() => api.deleteRole(role.id), t('up.deleted', { name: role.name }))) await load()
  }

  async function setGuestUnlimited(guest: GuestSummary, unlimited: boolean) {
    if (await attempt(() => api.updateUser(guest.id, { unlimited }))) {
      guests = guests.map((g) => (g.id === guest.id ? { ...g, unlimited } : g))
    }
  }

  async function removeGuest(guest: GuestSummary) {
    if (!confirm(t('up.remove_guest_q', { name: guest.name }))) return
    if (await attempt(() => api.deleteUser(guest.id))) guests = guests.filter((g) => g.id !== guest.id)
  }

  const roleName = (id: number) => roles.find((r) => r.id === id)?.name ?? t('up.listener')
  const when = (ms: number) =>
    new Date(ms).toLocaleString(locale.code, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
  const day = (ms: number) => new Date(ms).toLocaleDateString(locale.code, { month: 'short', day: 'numeric' })

  const permissionLabels: [keyof Permissions, string, string][] = $derived([
    ['go', t('up.p_go'), t('up.p_go_h')],
    ['go_remote', t('up.p_remote'), t('up.p_remote_h')],
    ['require_two_step', t('up.p_two_step'), t('up.p_two_step_h')],
    ['downloads', t('up.p_downloads'), t('up.p_downloads_h')],
    ['convert', t('up.p_convert'), t('up.p_convert_h')],
    ['share_playlists', t('up.p_share'), t('up.p_share_h')],
    ['submit_music', t('up.p_submit'), t('up.p_submit_h')],
    ['submit_without_review', t('up.p_no_review'), t('up.p_no_review_h')],
    ['unlimited_requests', t('up.p_unlimited'), t('up.p_unlimited_h')],
    ['auto_mix', t('up.p_auto_mix'), t('up.p_auto_mix_h')],
  ])
  const dependsOn: Partial<Record<keyof Permissions, keyof Permissions>> = {
    go_remote: 'go',
    require_two_step: 'go',
    submit_without_review: 'submit_music',
  }

  function summary(role: AccountRole): string {
    const can = permissionLabels.filter(([key]) => role[key]).map(([, label]) => label)
    return can.length ? t('up.can', { list: can.join(', ') }) : t('up.requests_only')
  }
</script>

{#if pending.length}
  <section class="card">
    <h2>{t('up.waiting')}</h2>
    {#each pending as user (user.id)}
      <div class="user">
        <div class="grow">
          <div>{user.username}</div>
          <div class="muted small">{t('up.signed_up', { when: when(user.created_at) })}</div>
          <div class="user-controls">
            <select
              value={accessOf(user)}
              aria-label={t('up.role_for', { name: user.username })}
              onchange={(e) => setAccess(user, e.currentTarget.value)}
            >
              {#each roles as role (role.id)}<option value="role:{role.id}">{role.name}</option>{/each}
            </select>
            <button class="small-button" onclick={() => editUsername(user)}>{t('up.change_username')}</button>
            <button class="primary small-button" onclick={() => approve(user)}>{t('up.approve')}</button>
            <button class="small-button" onclick={() => deny(user)}>{t('up.deny')}</button>
          </div>
        </div>
      </div>
    {/each}
  </section>
{/if}

<section class="card wide">
  <h2>{t('up.accounts')}</h2>
  {#each accounts as user (user.id)}
    <div class="user" class:off={user.status === 'disabled'}>
      <div class="grow">
        <div>
          {user.username}
          {#if user.id === app.me?.id}<span class="muted small">{t('up.you')}</span>{/if}
          {#if user.status === 'disabled'}<span class="badge">{t('up.turned_off')}</span>{/if}
          {#if user.two_step}<span class="badge">{t('up.two_step')}</span>{/if}
          {#if locked.includes(user.id)}<span class="badge">{t('up.locked')}</span>{/if}
        </div>
        <div class="user-controls">
          <select
            value={accessOf(user)}
            disabled={user.id === app.me?.id}
            aria-label={t('up.role_for', { name: user.username })}
            onchange={(e) => setAccess(user, e.currentTarget.value)}
          >
            <option value="admin">{t('up.admin')}</option>
            {#each roles as role (role.id)}<option value="role:{role.id}">{role.name}</option>{/each}
          </select>
          {#if user.role !== 'admin'}
            <label class="switch">
              <input type="checkbox" checked={user.unlimited} onchange={(e) => setUnlimited(user, e.currentTarget.checked)} />
              {t('up.no_limit')}
            </label>
          {/if}
          <button class="small-button" onclick={() => showDevices(user)}>{t('up.devices')}</button>
          {#if user.id !== app.me?.id}
            <button class="small-button" onclick={() => editUsername(user)}>{t('up.change_username')}</button>
          {/if}
          <button class="small-button" onclick={() => resetPassword(user)}>{t('up.reset_password')}</button>
          {#if user.two_step && user.id !== app.me?.id}
            <button class="small-button" onclick={() => resetTwoStep(user)}>{t('up.turn_off_two_step')}</button>
          {/if}
          {#if locked.includes(user.id)}
            <button class="small-button" onclick={() => unlock(user)}>{t('up.unlock')}</button>
          {/if}
          {#if user.id !== app.me?.id}
            {#if user.status === 'disabled'}
              <button class="small-button" onclick={() => setStatus(user, 'active', t('up.can_sign_in_again', { name: user.username }))}
                >{t('up.turn_on')}</button
              >
            {:else}
              <button class="small-button" onclick={() => setStatus(user, 'disabled', t('up.cant_sign_in', { name: user.username }))}
                >{t('up.turn_off')}</button
              >
            {/if}
          {/if}
        </div>
      </div>
      {#if user.id !== app.me?.id}
        <button class="icon" aria-label={t('up.delete_user', { name: user.username })} onclick={() => removeUser(user)}><Icon name="close" /></button>
      {/if}
    </div>
  {/each}
  <form class="new-user" onsubmit={addUser}>
    <input placeholder={t('up.username')} bind:value={newUser.username} maxlength="32" autocapitalize="none" required />
    <input placeholder={t('up.password')} type="password" bind:value={newUser.password} autocomplete="new-password" minlength="8" required />
    <input placeholder={t('up.password_again')} type="password" bind:value={newUser.confirm} autocomplete="new-password" required />
    <select bind:value={newUser.access} aria-label={t('up.role_new')}>
      {#each roles as role (role.id)}<option value="role:{role.id}">{role.name}</option>{/each}
      <option value="admin">{t('up.admin')}</option>
    </select>
    <button class="primary">{t('up.add_account')}</button>
  </form>
</section>

<section class="card wide">
  <h2>{t('up.roles')}</h2>
  <p class="small muted">{t('up.roles_note')}</p>
  {#each roles as role (role.id)}
    <div class="user">
      <div class="grow">
        <div>
          {role.name}
          <span class="muted small">{t('up.n_accounts', { n: role.accounts })}</span>
        </div>
        <div class="muted small">{summary(role)}</div>
      </div>
      <button class="small-button" onclick={() => editRole(role)}>{t('up.edit')}</button>
      {#if !role.builtin}
        <button class="icon" aria-label={t('up.delete_user', { name: role.name })} onclick={() => removeRole(role)}><Icon name="close" /></button>
      {/if}
    </div>
  {/each}
  <button class="small-button add" onclick={() => editRole()}>{t('up.new_role')}</button>
</section>

{#if signup}
  <section class="card">
    <h2>{t('up.signups')}</h2>
    <label>
      {t('up.who_creates')}
      <select bind:value={signup.mode} onchange={saveSignup}>
        <option value="closed">{t('up.closed')}</option>
        <option value="review">{t('up.review')}</option>
        <option value="open">{t('up.open')}</option>
      </select>
    </label>
    {#if signup.mode !== 'closed'}
      <label>
        {t('up.new_get')}
        <select bind:value={signup.role_id} onchange={saveSignup}>
          {#each roles as role (role.id)}<option value={role.id}>{role.name}</option>{/each}
        </select>
      </label>
      {#if signup.mode === 'open'}
        <p class="small muted">{t('up.open_note')}</p>
      {/if}
    {/if}
  </section>
{/if}

<section class="card">
  <h2>{t('up.invites')}</h2>
  <p class="small muted">{t('up.invites_note')}</p>
  <form class="invite-form" onsubmit={makeInvite}>
    <label>
      {t('up.role')}
      <select bind:value={newInvite.role_id}>
        {#each roles as role (role.id)}<option value={role.id}>{role.name}</option>{/each}
      </select>
    </label>
    <label>
      {t('up.can_be_used')}
      <select bind:value={newInvite.uses}>
        <option value="1">{t('up.once')}</option>
        <option value="5">{t('up.times', { n: 5 })}</option>
        <option value="25">{t('up.times', { n: 25 })}</option>
        <option value="any">{t('up.any_times')}</option>
      </select>
    </label>
    <label>
      {t('up.works_for')}
      <select bind:value={newInvite.days}>
        <option value="1">{t('up.days', { n: 1 })}</option>
        <option value="7">{t('up.days', { n: 7 })}</option>
        <option value="30">{t('up.days', { n: 30 })}</option>
        <option value="never">{t('up.until_cancel')}</option>
      </select>
    </label>
    <label>
      {t('up.note')}
      <input bind:value={newInvite.note} maxlength="100" placeholder={t('up.who_for')} />
    </label>
    <button class="primary">{t('up.make_invite')}</button>
  </form>
  {#if inviteLink}
    <div class="new-link">
      <p class="small">{t('up.copy_now')}</p>
      <input readonly value={inviteLink} onfocus={(e) => e.currentTarget.select()} aria-label={t('up.invite_link')} />
      <button class="small-button" onclick={copyLink}>{t('up.copy_link')}</button>
    </div>
  {/if}
  {#each invites as invite (invite.id)}
    <div class="user">
      <div class="grow">
        <div>{invite.note || roleName(invite.role_id)} <span class="muted small">{invite.role_name}</span></div>
        <div class="muted small">
          {invite.uses_left === null ? t('up.any_uses') : t('up.uses_left', { n: invite.uses_left })},
          {invite.expires_at === null ? t('up.no_end') : t('up.until', { date: day(invite.expires_at) })}
          {#if invite.used}{t('up.used', { n: invite.used })}{/if}
        </div>
      </div>
      <button class="icon" aria-label={t('up.cancel_invite')} onclick={() => removeInvite(invite)}><Icon name="close" /></button>
    </div>
  {/each}
</section>

<section class="card">
  <h2>{t('up.guests')}</h2>
  <p class="small muted">{t('up.guests_note')}</p>
  {#each guests as guest (guest.id)}
    <div class="user">
      <div class="grow">
        <div>{guest.name} <span class="muted small">{t('up.n_requests', { n: guest.requests })}</span></div>
        <div class="muted small mono">{guest.mac ?? t('up.no_mac', { ip: guest.last_ip ?? t('up.unknown') })}</div>
        {#if guest.names !== guest.name}<div class="muted small">{t('up.aka', { names: guest.names })}</div>{/if}
        <div class="muted small">{t('up.last_seen', { when: when(guest.last_seen) })}</div>
      </div>
      <label class="switch">
        <input type="checkbox" checked={guest.unlimited} onchange={(e) => setGuestUnlimited(guest, e.currentTarget.checked)} />
        {t('up.no_limit')}
      </label>
      <button class="icon" aria-label={t('up.remove_guest', { name: guest.name })} onclick={() => removeGuest(guest)}><Icon name="close" /></button>
    </div>
  {:else}
    <p class="muted">{t('up.no_guests')} <a href="#/admin/rules">{t('admin.rules')}</a>.</p>
  {/each}
</section>

{#if editing}
  <Sheet title={editing.id ? t('up.edit_role', { name: editing.name }) : t('up.new_role')} onclose={() => (editing = null)}>
    <form class="role-form" onsubmit={saveRole}>
      <label>
        {t('up.name')}
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
        {t('up.streams')}
        <select bind:value={editing.max_streams} disabled={!editing.go}>
          <option value={1}>1</option>
          <option value={2}>2</option>
          <option value={3}>3</option>
          <option value={5}>5</option>
          <option value={0}>{t('up.no_limit')}</option>
        </select>
      </label>
      <button class="primary">{editing.id ? t('common.save') : t('up.add_role')}</button>
    </form>
  </Sheet>
{/if}

{#if editingUsername}
  <Sheet title={t('up.change_username')} onclose={() => (editingUsername = null)}>
    <form class="username-form" onsubmit={saveUsername}>
      <label>
        {t('up.new_username', { name: editingUsername.username })}
        <input bind:value={usernameDraft} maxlength="32" autocomplete="off" autocapitalize="none" required />
      </label>
      <button class="primary">{t('up.save_username')}</button>
    </form>
  </Sheet>
{/if}

{#if devicesFor}
  <Sheet title={t('up.their_devices', { name: devicesFor.username })} onclose={() => (devicesFor = null)}>
    {#each devices as device (device.id)}
      <div class="user">
        <div class="grow">
          <div>{deviceName(device.user_agent)}</div>
          <div class="muted small">
            {t('up.device_line', {
              ip: device.ip ?? t('up.unknown_address'),
              used: when(device.last_seen_at),
              signed: day(device.created_at),
            })}
          </div>
        </div>
        <button class="small-button" onclick={() => signOutDevice(device)}>{t('up.sign_out')}</button>
      </div>
    {:else}
      <p class="muted">{t('up.nowhere')}</p>
    {/each}
    {#if devices.length > 1}
      <button class="signout-all" onclick={signOutEverywhere}>{t('up.sign_out_all')}</button>
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

  .new-user > :first-child,
  .new-user > select,
  .new-user > button { grid-column: 1 / -1; }

  .username-form {
    display: grid;
    gap: 12px;
  }

  .username-form input { width: 100%; }

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
