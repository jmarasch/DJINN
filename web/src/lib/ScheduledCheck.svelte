<script lang="ts">
  // The nightly library check: on or off, its hour, and what the last one
  // found, to read in the morning.
  import { api } from './api'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { CheckSummary, ScheduledCheck } from './types'

  let check = $state<ScheduledCheck | null>(null)

  $effect(() => {
    api
      .scheduledCheck()
      .then((c) => (check = c))
      .catch((e) => toast(e.message, true))
  })

  async function save() {
    if (!check) return
    try {
      check = await api.saveScheduledCheck(check.on, check.hour)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('sc.save_failed'), true)
    }
  }

  const hourLabel = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString(locale.code, { hour: 'numeric' })
  const n = (x: number) => x.toLocaleString(locale.code)
  const when = (ms: number) =>
    new Date(ms).toLocaleString(locale.code, { weekday: 'long', hour: 'numeric', minute: '2-digit' })
  /** "3 new" or "2 fewer" after a count, when it changed. */
  const change = (x: number) =>
    x > 0 ? t('sc.change_new', { n: n(x) }) : x < 0 ? t('sc.change_fewer', { n: n(-x) }) : ''
  function lines(s: CheckSummary): string[] {
    return [
      s.added || s.removed
        ? t('sc.added_gone', { added: t('sc.n_songs', { n: s.added }), removed: t('sc.n_songs', { n: s.removed }) })
        : t('sc.no_change'),
      t('sc.problems', { n: s.trouble, change: change(s.trouble_change) }),
      t('sc.copies', { n: s.duplicates, change: change(s.duplicates_change) }),
      t('sc.reported', { n: s.reports, change: change(s.reports_change) }),
    ]
  }
</script>

<section class="card">
  <h2>{t('sc.title')}</h2>
  {#if check}
    <label class="switch big">
      <input type="checkbox" bind:checked={check.on} onchange={save} />
      {t('sc.on')}
    </label>
    <label>
      {t('sc.at')}
      <select bind:value={check.hour} onchange={save} disabled={!check.on}>
        {#each Array.from({ length: 24 }, (_, h) => h) as h (h)}
          <option value={h}>{hourLabel(h)}</option>
        {/each}
      </select>
      <span class="small muted">
        {t('sc.note')}
      </span>
    </label>
    {#if check.last}
      <div>
        <p class="small muted">{t('sc.last', { when: when(check.last.at) })}</p>
        <ul class="small">
          {#each lines(check.last) as line, i (i)}<li>{line}</li>{/each}
        </ul>
      </div>
    {/if}
  {/if}
</section>

<style>
  ul {
    margin: 4px 0 0;
    padding-left: 20px;
  }
</style>
