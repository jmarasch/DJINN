<script lang="ts">
  // Admin > Access > Protected Areas: how strict the lock on library changes
  // is, whether this session has it open, and the audit trail.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { askToUnlock, unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { AuditEntry, ProtectedLevel, ProtectedStatus } from './types'

  const levels: { id: ProtectedLevel; label: string; about: string }[] = $derived([
    { id: 'off', label: t('pa.off'), about: t('pa.off_about') },
    { id: 'medium', label: t('pa.medium'), about: t('pa.medium_about') },
    { id: 'max', label: t('pa.max'), about: t('pa.max_about') },
  ])

  let status = $state<ProtectedStatus | null>(null)
  let entries = $state<AuditEntry[]>([])
  let total = $state(0)
  let page = $state(0)

  // Changing the level.
  let target = $state<ProtectedLevel | null>(null)
  let password = $state('')
  let code = $state('')
  let word = $state('')
  let busy = $state(false)

  const n = (x: number) => x.toLocaleString(locale.code)

  async function refresh() {
    try {
      status = await api.protectedStatus()
      if (target === null) target = status.level
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.load_failed'), true)
    }
  }

  async function loadAudit(to = page) {
    try {
      const found = await api.auditPage(Math.max(0, to) * 50)
      entries = found.entries
      total = found.total
      page = to
    } catch {
      // Shown next time.
    }
  }

  onMount(() => {
    void refresh()
    void loadAudit(0)
    const timer = setInterval(() => void refresh(), 10_000)
    return () => clearInterval(timer)
  })

  const needCode = $derived(status?.level === 'max' && status.two_step)
  const changing = $derived(status !== null && target !== null && target !== status.level)

  async function apply() {
    if (!target || !status) return
    busy = true
    try {
      await api.setProtectedLevel(target, password, code, word)
      toast(t('common.saved'))
      password = code = word = ''
      target = null
      await refresh()
      await loadAudit(0)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('lf.change_failed'), true)
      code = ''
    } finally {
      busy = false
    }
  }

  async function dismissRecovery() {
    await api.dismissRecovery().catch(() => {})
    await refresh()
  }

  async function lockNow() {
    await api.protectedLeave().catch(() => {})
    await refresh()
    await loadAudit(0)
  }

  async function unlock() {
    if (await askToUnlock()) {
      await refresh()
      await loadAudit(0)
    }
  }

  async function toggleAudit(on: boolean) {
    try {
      await unlocked(() => api.setAudit(on))
      await refresh()
      await loadAudit(0)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('lf.change_failed'), true)
      await refresh()
    }
  }

  async function purge() {
    if (!confirm(t('pa.purge_q', { n: n(status?.audit_count ?? total) }))) return
    try {
      const done = await unlocked(() => api.purgeAudit())
      toast(t('pa.purged', { n: n(done.removed) }))
      await refresh()
      await loadAudit(0)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('pa.purge_failed'), true)
    }
  }

  /** Audit kinds with wording (`pa.k_<kind>`); others show as they are. */
  const KINDS = [
    'entered',
    'entry_failed',
    'left',
    'level_changed',
    'audit_on',
    'audit_off',
    'audit_purged',
    'approved_submission',
    'account_unlocked',
    'recovery_used',
  ]
  const kindName = (kind: string) => (KINDS.includes(kind) ? t(`pa.k_${kind}`) : kind)
  const when = (ms: number) => new Date(ms).toLocaleString(locale.code)
</script>

<section class="card">
  <h3>{t('admin.menu.protected')}</h3>
  <p class="small muted">
    {t('pa.intro')}
  </p>
  {#if status?.recovery_used}
    <p class="small bad">
      {t('pa.recovery', { when: when(status.recovery_used.at), name: status.recovery_used.username })}
      <button class="link" onclick={dismissRecovery}>{t('pa.dismiss')}</button>
    </p>
  {/if}
  {#if status}
    <p class="small">
      {#if status.level === 'off'}
        {t('pa.lock_off')}
      {:else if status.open}
        {t('pa.open')}{#if status.seconds_left !== null}{t('pa.locks_in', { n: Math.ceil(status.seconds_left / 60) })}{/if}.
        <button class="link" onclick={lockNow}>{t('pa.lock_now')}</button>
      {:else}
        {t('pa.locked')} <button class="link" onclick={unlock}>{t('ed.unlock')}</button>
      {/if}
    </p>

    <h4>{t('pa.how_strict')}</h4>
    <div class="levels" role="radiogroup" aria-label={t('pa.how_strict_label')}>
      {#each levels as l (l.id)}
        <label class="level" class:on={target === l.id}>
          <input type="radio" name="level" value={l.id} bind:group={target} />
          <strong>{l.label}</strong>
          <span class="small muted">{l.about}</span>
          {#if status.level === l.id}<span class="small tag">{t('pa.now')}</span>{/if}
        </label>
      {/each}
    </div>
    {#if target === 'max' && !status.two_step}
      <p class="small bad">{t('pa.max_needs')}</p>
    {/if}
    {#if changing}
      <form
        class="change"
        onsubmit={(e) => {
          e.preventDefault()
          void apply()
        }}
      >
        {#if target === 'off'}
          <p class="small bad">
            {t('pa.off_warning')}
          </p>
          <label>
            {t('pa.type_off')}
            <input bind:value={word} autocomplete="off" />
          </label>
        {/if}
        <label>
          {t('pa.password')}
          <input type="password" autocomplete="current-password" bind:value={password} />
        </label>
        {#if needCode}
          <label>
            {t('pa.code')}
            <input inputmode="numeric" autocomplete="one-time-code" bind:value={code} />
          </label>
        {/if}
        <div class="actions">
          <button class="primary" disabled={busy || !password || (target === 'off' && word.trim() !== 'OFF')}>
            {t('pa.change_to', { level: levels.find((l) => l.id === target)?.label ?? '' })}
          </button>
          <button type="button" onclick={() => (target = status?.level ?? null)}>{t('common.cancel')}</button>
        </div>
      </form>
    {/if}
  {:else}
    <p class="small muted">{t('common.loading')}</p>
  {/if}
</section>

<section class="card">
  <h3>{t('pa.audit')}</h3>
  <p class="small muted">
    {t('pa.audit_note')}
  </p>
  {#if status}
    <label class="switch">
      <input type="checkbox" checked={status.audit_on} onchange={(e) => toggleAudit(e.currentTarget.checked)} />
      {t('pa.keep')}
    </label>
    <p class="small">
      {t('pa.records', { n: status.audit_count })}
      {#if status.audit_count > 0}<button class="danger small-button" onclick={purge}>{t('pa.delete_all')}</button>{/if}
    </p>
  {/if}
  {#if entries.length}
    <ul class="trail">
      {#each entries as e (e.id)}
        <li>
          <span class="small muted">{when(e.at)}</span>
          <strong>{kindName(e.kind)}</strong>
          <span class="small">{e.username}{e.ip ? t('pa.from', { ip: e.ip }) : ''}</span>
          {#if e.detail}<span class="small muted">{e.detail}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if total > 50}
      <div class="actions">
        <button class="small-button" disabled={page === 0} onclick={() => loadAudit(page - 1)}>{t('pa.newer')}</button>
        <span class="small muted">{t('pa.page', { n: page + 1, total: Math.ceil(total / 50) })}</span>
        <button class="small-button" disabled={(page + 1) * 50 >= total} onclick={() => loadAudit(page + 1)}>{t('pa.older')}</button>
      </div>
    {/if}
  {/if}
</section>

<style>
  .levels {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 8px 0;
  }

  .level {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 2px 10px;
    align-items: center;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 8px;
    cursor: pointer;
  }

  .level.on {
    border-color: var(--accent);
  }

  .level input {
    grid-row: 1 / span 2;
  }

  .level span:nth-of-type(1) {
    grid-column: 2;
  }

  .tag {
    grid-row: 1;
    grid-column: 3;
  }

  .change {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 8px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  .trail {
    list-style: none;
    margin: 8px 0;
    padding: 0;
  }

  .trail li {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    padding: 6px 0;
    border-top: 1px solid var(--line);
  }

  .bad {
    color: var(--danger, #d33);
  }
</style>
