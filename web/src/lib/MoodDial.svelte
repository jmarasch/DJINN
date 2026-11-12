<script lang="ts">
  // The mood dial on the Playing tab: everyone nudges the auto mix from
  // chill to hype. Your setting counts for a while; the party's mood is
  // everyone's together (and the schedule's resting point).
  import { onMount } from 'svelte'
  import { api } from './api'
  import { t } from './i18n.svelte'
  import { app, toast } from './state.svelte'

  onMount(() => {
    if (!app.mood) api.mood().then((mood) => (app.mood = mood)).catch(() => {})
  })

  const mine = $derived(app.mood?.votes[String(app.me?.id ?? -1)])
  const people = $derived(Object.keys(app.mood?.votes ?? {}).length)
  /** In-order playlists keep their order, so the dial has nothing to do. */
  const steers = $derived(!!app.player.playlist && (app.player.playlist.shuffle || app.player.playlist.id < 0))
  const shown = $derived(!!app.mood?.on && app.mood.works && steers)

  let value = $state(0)
  let touched = $state(false)
  $effect(() => {
    if (!touched) value = mine ?? app.mood?.value ?? 0
  })

  let timer: ReturnType<typeof setTimeout>
  function send() {
    touched = true
    clearTimeout(timer)
    timer = setTimeout(async () => {
      try {
        app.mood = await api.nudgeMood(value)
      } catch (e) {
        toast(e instanceof Error ? e.message : t('mood.failed'), true)
      } finally {
        touched = false
      }
    }, 300)
  }

  /** The party's mood in words. */
  function word(v: number): string {
    if (v <= -0.6) return t('mood.chill')
    if (v <= -0.2) return t('mood.easy')
    if (v < 0.2) return t('mood.middle')
    if (v < 0.6) return t('mood.lively')
    return t('mood.hype')
  }
</script>

{#if shown && app.mood}
  <section class="mood" aria-label={t('mood.title')}>
    <div class="head">
      <span class="label">{t('mood.title')}</span>
      <span class="small muted">
        {t('mood.party_is', { mood: word(app.mood.value) })}{people
          ? ` · ${t('mood.people', { n: people })}`
          : ''}
      </span>
    </div>
    <input
      type="range"
      min="-1"
      max="1"
      step="0.1"
      bind:value
      oninput={send}
      aria-label={t('mood.your_setting')}
      aria-valuetext={word(value)}
      style:--party="{((app.mood.value + 1) / 2) * 100}%"
    />
    <div class="ends small muted">
      <span>{t('mood.chill')}</span>
      <span>{t('mood.hype')}</span>
    </div>
  </section>
{/if}

<style>
  .mood {
    width: 100%;
    margin-top: 16px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--surface);
  }

  .head,
  .ends {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
  }

  .label {
    font-weight: 600;
  }

  input {
    width: 100%;
    margin: 10px 0 2px;
    accent-color: var(--accent);
    /* Where the party is, under your own setting. */
    background: linear-gradient(to right, transparent calc(var(--party) - 1px), var(--muted) 0, var(--muted) calc(var(--party) + 1px), transparent 0);
  }
</style>
