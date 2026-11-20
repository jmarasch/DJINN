<script lang="ts">
  // Admin > Activity and Logs > Sign-In Log: every sign-in to DJINN and DJINN
  // GO, a page at a time, with clearing old ones.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { deviceName } from './format'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { SignIn } from './types'

  const PAGE = 50
  let items = $state<SignIn[]>([])
  let total = $state(0)
  let page = $state(0)
  let failed = $state(false)
  let keepDays = $state(30)
  let confirming = $state(false)

  const pages = $derived(Math.max(1, Math.ceil(total / PAGE)))

  async function load() {
    try {
      const got = await api.signIns(page * PAGE, PAGE)
      items = got.items
      total = got.total
      failed = false
      if (page > 0 && page >= Math.ceil(total / PAGE)) {
        page = Math.max(0, Math.ceil(total / PAGE) - 1)
        await load()
      }
    } catch {
      failed = true
    }
  }

  onMount(() => void load())

  function go(to: number) {
    page = Math.min(Math.max(0, to), pages - 1)
    void load()
  }

  async function clear() {
    try {
      const { cleared } = await api.clearSignIns(keepDays)
      toast(t('si.cleared', { n: cleared }))
      confirming = false
      page = 0
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('si.clear_failed'), true)
    }
  }

  /** Sign-in outcomes with wording (`si.<outcome>`); others show as they are. */
  const OUTCOMES = ['ok', 'wrong_password', 'locked_out', 'account_locked', 'waiting', 'turned_off', 'wrong_code', 'no_go', 'not_home']
  const outcome = (o: string) => (OUTCOMES.includes(o) ? t(`si.${o}`) : o)
  const when = (ms: number) =>
    new Date(ms).toLocaleString(locale.code, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
</script>

<section class="card">
  <h2>{t('admin.menu.signins')}</h2>
  <p class="small muted">
    {t('si.intro')}
  </p>
  {#if failed}
    <p class="error">{t('si.load_failed')}</p>
  {:else}
    {#each items as s (s.id)}
      <div class="row" class:bad={s.outcome !== 'ok'}>
        <div>
          <b>{s.username}</b> {outcome(s.outcome)} <span class="muted small">{s.via === 'go' ? 'GO' : t('si.party')}</span>
        </div>
        <div class="muted small">{when(s.at)}, {s.ip ?? t('si.unknown_address')}, {deviceName(s.user_agent)}</div>
      </div>
    {:else}
      <p class="muted">{t('si.none')}</p>
    {/each}
    {#if total > PAGE}
      <div class="pager">
        <button class="small-button" onclick={() => go(page - 1)} disabled={page === 0}>{t('pa.newer')}</button>
        <span class="small muted"
          >{t('si.page', { n: page + 1, pages, total: total.toLocaleString(locale.code) })}</span
        >
        <button class="small-button" onclick={() => go(page + 1)} disabled={page >= pages - 1}>{t('pa.older')}</button>
      </div>
    {/if}
  {/if}
</section>

<section class="card">
  <h3>{t('si.clear_old')}</h3>
  <label class="keep">
    {t('si.keep')}
    <select bind:value={keepDays}>
      <option value={1}>{t('si.day')}</option>
      <option value={7}>{t('si.days', { n: 7 })}</option>
      <option value={30}>{t('si.days', { n: 30 })}</option>
      <option value={0}>{t('si.nothing')}</option>
    </select>
  </label>
  {#if confirming}
    <p class="small">
      {keepDays === 0
        ? t('si.all_removed')
        : keepDays === 1
          ? t('si.older_day')
          : t('si.older_days', { n: keepDays })}
      {t('si.not_affected')}
    </p>
    <button class="danger" onclick={clear}>{t('st.clear_them')}</button>
    <button onclick={() => (confirming = false)}>{t('common.cancel')}</button>
  {:else}
    <button onclick={() => (confirming = true)}>{t('common.clear')}</button>
  {/if}
</section>

<style>
  .row {
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .row.bad b {
    color: var(--bad);
  }

  .pager {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: 12px;
  }

  .keep {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
</style>
