<script lang="ts">
  // Admin > Smart Play > Party schedule: time rules like "a quiet playlist
  // until 9, then wild play, turned up". Each rule is applied once as it
  // starts, so changes made by hand during it stay.
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { PartySchedule, PlaylistListItem, ScheduleRule } from './types'

  let room = $state(1)
  let schedule = $state<PartySchedule | null>(null)
  let playlists = $state<PlaylistListItem[]>([])
  let saving = $state(false)
  let changed = $state(false)

  // Monday first; 1 January 2024 was a Monday.
  const DAYS = $derived(
    Array.from({ length: 7 }, (_, d) => new Date(2024, 0, 1 + d).toLocaleDateString(locale.code, { weekday: 'short' })),
  )
  const MOODS = $derived([
    { value: null, label: t('sch.mood_middle') },
    { value: -0.8, label: t('sch.chill') },
    { value: -0.4, label: t('sch.easy') },
    { value: 0.4, label: t('sch.lively') },
    { value: 0.8, label: t('sch.hype') },
  ])
  /** Rooms that play on their own (linked ones follow their leader's rules). */
  const rooms = $derived(app.rooms.filter((r) => r.linked_to == null))

  $effect(() => {
    const which = room
    load(which).catch((e) => toast(e instanceof Error ? e.message : t('sch.load_failed'), true))
  })

  async function load(which: number) {
    const [found, lists] = await Promise.all([api.partySchedule(which), api.playlists()])
    if (which !== room) return
    schedule = found
    playlists = lists
    changed = false
  }

  const time = (minutes: number) =>
    `${String(Math.floor(minutes / 60)).padStart(2, '0')}:${String(minutes % 60).padStart(2, '0')}`
  const minutes = (text: string) => {
    const [h, m] = text.split(':').map(Number)
    return Number.isFinite(h) && Number.isFinite(m) ? h * 60 + m : 0
  }

  function edit(change: () => void) {
    change()
    changed = true
  }

  function addRule(rule?: Partial<ScheduleRule>) {
    edit(() =>
      schedule?.rules.push({
        days: [],
        from: 18 * 60,
        until: 21 * 60,
        play: { kind: 'keep' },
        mood: null,
        volume: null,
        ...rule,
      }),
    )
  }

  function example() {
    if (!schedule) return
    addRule({ from: 18 * 60, until: 21 * 60, mood: -0.8, volume: 45 })
    addRule({ from: 21 * 60, until: 2 * 60, play: { kind: 'wild' }, mood: 0.8, volume: 75 })
    schedule.on = true
  }

  function toggleDay(rule: ScheduleRule, day: number) {
    edit(() => {
      rule.days = rule.days.includes(day) ? rule.days.filter((d) => d !== day) : [...rule.days, day].sort()
    })
  }

  /** The play choice as one select value. */
  const playValue = (rule: ScheduleRule) => (rule.play.kind === 'playlist' ? `playlist:${rule.play.id}` : rule.play.kind)
  function setPlay(rule: ScheduleRule, value: string) {
    edit(() => {
      rule.play = value.startsWith('playlist:')
        ? { kind: 'playlist', id: Number(value.slice(9)) }
        : value === 'wild'
          ? { kind: 'wild' }
          : { kind: 'keep' }
    })
  }

  async function save() {
    if (!schedule) return
    saving = true
    try {
      await api.savePartySchedule(room, schedule)
      toast(t('sch.saved'))
      await load(room)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('sch.save_failed'), true)
    } finally {
      saving = false
    }
  }

  /** "Every day, 18:00 to 21:00" for a screen reader's heading. */
  const summary = (rule: ScheduleRule) =>
    t('sch.summary', {
      days: rule.days.length ? rule.days.map((d) => DAYS[d]).join(', ') : t('sch.every_day'),
      from: time(rule.from),
      until: time(rule.until),
    })
</script>

{#if schedule}
  <section class="card">
    <h2>{t('sch.title')}</h2>
    <p class="small muted">
      {t('sch.intro')}
    </p>
    {#if rooms.length > 1}
      <label class="room">
        {t('sch.room')}
        <select bind:value={room}>
          {#each rooms as r (r.id)}<option value={r.id}>{r.name}</option>{/each}
        </select>
      </label>
    {/if}
    <label class="switch">
      <input type="checkbox" checked={schedule.on} onchange={(e) => edit(() => (schedule!.on = e.currentTarget.checked))} />
      {t('sch.follow')}
    </label>

    {#each schedule.rules as rule, i (i)}
      <div class="rule" aria-label={summary(rule)} role="group">
        <div class="rule-head">
          <span class="small muted">{t('sch.rule', { n: i + 1 })}</span>
          {#if schedule.holding === i && !changed}<span class="badge">{t('sch.now')}</span>{/if}
          <button class="icon-only" aria-label={t('sch.remove', { n: i + 1 })} onclick={() => edit(() => schedule!.rules.splice(i, 1))}>
            <Icon name="close" />
          </button>
        </div>
        <div class="days" role="group" aria-label={t('sch.days')}>
          {#each DAYS as day, d (d)}
            <button class:chosen={rule.days.includes(d)} aria-pressed={rule.days.includes(d)} onclick={() => toggleDay(rule, d)}>
              {day}
            </button>
          {/each}
          {#if !rule.days.length}<span class="small muted">{t('sch.every_day')}</span>{/if}
        </div>
        <div class="times">
          <label>{t('sch.from')} <input type="time" value={time(rule.from)} onchange={(e) => edit(() => (rule.from = minutes(e.currentTarget.value)))} /></label>
          <label>{t('sch.until')} <input type="time" value={time(rule.until)} onchange={(e) => edit(() => (rule.until = minutes(e.currentTarget.value)))} /></label>
        </div>
        <label class="field">
          {t('sch.auto_mix')}
          <select value={playValue(rule)} onchange={(e) => setPlay(rule, e.currentTarget.value)}>
            <option value="keep">{t('sch.keep')}</option>
            <option value="wild">{t('sch.wild')}</option>
            {#each playlists as list (list.id)}
              <option value="playlist:{list.id}">{t('sch.playlist', { name: list.name })}</option>
            {/each}
          </select>
        </label>
        <label class="field">
          {t('sch.mood')}
          <select
            value={String(rule.mood)}
            onchange={(e) => edit(() => (rule.mood = e.currentTarget.value === 'null' ? null : Number(e.currentTarget.value)))}
          >
            {#each MOODS as mood (String(mood.value))}<option value={String(mood.value)}>{mood.label}</option>{/each}
          </select>
        </label>
        <div class="volume">
          <label class="switch">
            <input
              type="checkbox"
              checked={rule.volume != null}
              onchange={(e) => edit(() => (rule.volume = e.currentTarget.checked ? (app.player.volume ?? 60) : null))}
            />
            {t('sch.volume')}
          </label>
          {#if rule.volume != null}
            <input
              type="range"
              min="0"
              max="100"
              step="5"
              value={rule.volume}
              oninput={(e) => edit(() => (rule.volume = Number(e.currentTarget.value)))}
              aria-label={t('sch.volume_for', { n: i + 1 })}
            />
            <span class="small muted">{rule.volume}%</span>
          {/if}
        </div>
      </div>
    {/each}

    <div class="buttons">
      <button class="small-button" disabled={schedule.rules.length >= 24} onclick={() => addRule()}>{t('sch.add')}</button>
      {#if !schedule.rules.length}
        <button class="small-button" onclick={example}>{t('sch.example')}</button>
      {/if}
      <button class="small-button primary" disabled={!changed || saving} onclick={save}>
        {saving ? t('common.saving') : t('common.save')}
      </button>
    </div>
  </section>
{/if}

<style>
  .room,
  .field {
    display: grid;
    gap: 4px;
    margin: 10px 0;
    font-size: 0.875rem;
  }

  .rule {
    margin: 14px 0;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: 12px;
  }

  .rule-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .rule-head .icon-only {
    margin-left: auto;
  }

  .days {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 8px 0;
  }

  .days button {
    min-height: 32px;
    padding: 4px 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface-2);
  }

  .days button.chosen {
    border-color: var(--accent);
    color: var(--accent);
  }

  .times {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .times label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.875rem;
  }

  .volume {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .volume input[type='range'] {
    flex: 1;
    min-width: 120px;
    accent-color: var(--accent);
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
  }
</style>
