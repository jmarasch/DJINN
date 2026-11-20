<script lang="ts">
  // A node's speaker settings: which speaker, delay, trim, EQ, and what to
  // do when Main is gone. Used in Admin > Nodes and on the node's own page;
  // both save to Main, which keeps them.
  import { t } from './i18n.svelte'
  import EqualizerPanel from './EqualizerPanel.svelte'
  import NodeScreenSettings from './NodeScreenSettings.svelte'
  import type { EqualizerSettings, NodeAudio, NodeSpeaker } from './types'

  let {
    audio,
    speakers,
    save,
  }: {
    audio: NodeAudio
    speakers: NodeSpeaker[]
    /** Saves the whole set; rejects with a message if it can't. */
    save: (audio: NodeAudio) => Promise<void>
  } = $props()

  // An editable copy, saved as it changes (taken once, on purpose).
  // svelte-ignore state_referenced_locally
  let draft = $state<NodeAudio>(structuredClone($state.snapshot(audio)))
  let error = $state('')
  let timer: ReturnType<typeof setTimeout>

  function changed(delay = 400) {
    clearTimeout(timer)
    timer = setTimeout(async () => {
      try {
        await save($state.snapshot(draft))
        error = ''
      } catch (e) {
        error = e instanceof Error ? e.message : t('common.save_failed_short')
      }
    }, delay)
  }

  /** Most speakers one node plays to, the first included. */
  const MAX_OUTPUTS = 6

</script>

{#snippet speakerBlock(o: { device: string | null; delay_ms: number; trim: number }, title: string, remove?: () => void)}
  {@const others = [draft.device, ...(draft.outputs ?? []).map((x) => x.device)].filter((d) => d && d !== o.device)}
  <div class="output">
    <div class="output-head">
      <b>{title}</b>
      {#if remove}<button class="small-button danger" onclick={remove}>{t('common.remove')}</button>{/if}
    </div>
    <label>
      {t('nsp.speaker')}
      <select
        value={o.device ?? ''}
        onchange={(e) => {
          o.device = e.currentTarget.value || null
          changed(0)
        }}
      >
        <option value="">{t('nsp.default')}</option>
        {#each speakers.filter((s) => (!s.is_default || s.id === o.device) && !others.includes(s.id)) as speaker (speaker.id)}
          <option value={speaker.id}>{speaker.name}</option>
        {/each}
        {#if o.device && !speakers.some((s) => s.id === o.device)}
          <option value={o.device}>{t('nsp.not_connected')}</option>
        {/if}
      </select>
    </label>
    <label>
      {t('nsp.delay', { ms: o.delay_ms })}
      <input type="range" min="0" max="1000" step="5" bind:value={o.delay_ms} oninput={() => changed()} />
    </label>
    <label>
      {t('nsp.trim', { pct: Math.round(o.trim * 100) })}
      <input type="range" min="0" max="1" step="0.01" bind:value={o.trim} oninput={() => changed()} />
    </label>
  </div>
{/snippet}

<div class="speaker">
  {@render speakerBlock(draft, (draft.outputs ?? []).length ? t('nsp.speaker_n', { n: 1 }) : t('nsp.speaker'))}
  {#each draft.outputs ?? [] as output, i (i)}
    {@render speakerBlock(output, t('nsp.speaker_n', { n: i + 2 }), () => {
      draft.outputs = (draft.outputs ?? []).filter((_, j) => j !== i)
      changed(0)
    })}
  {/each}
  {#if (draft.outputs ?? []).length < MAX_OUTPUTS - 1}
    <div>
      <button
        onclick={() => {
          draft.outputs = [...(draft.outputs ?? []), { device: null, delay_ms: draft.delay_ms, trim: 1 }]
          changed(0)
        }}
      >
        {t('nsp.add')}
      </button>
    </div>
  {/if}
  <p class="small muted">
    {t('nsp.note')}
  </p>
  <label>
    {t('nsp.offline')}
    <select bind:value={draft.offline} onchange={() => changed(0)}>
      <option value="stop">{t('nsp.stop')}</option>
      <option value="cached">{t('nsp.cached')}</option>
    </select>
  </label>
  <NodeScreenSettings
    canFollow
    screen={draft.screen ?? ''}
    visual={draft.visual ?? null}
    onchange={(screen, visual) => {
      draft.screen = screen
      draft.visual = visual
      changed(0)
    }}
  />
  <p class="small muted">{t('nsp.screen_note')}</p>
  <label class="check">
    <input
      type="checkbox"
      checked={!draft.free_run}
      onchange={(e) => {
        draft.free_run = !e.currentTarget.checked
        changed(0)
      }}
    />
    {t('nsp.in_step')}
  </label>
  <p class="small muted">
    {t('nsp.in_step_note')}
  </p>
  <label>
    {t('nsp.ready', { n: draft.cache_ahead ?? 12 })}
    <input
      type="range"
      min="1"
      max="50"
      value={draft.cache_ahead ?? 12}
      oninput={(e) => {
        draft.cache_ahead = Number(e.currentTarget.value)
        changed()
      }}
    />
  </label>
  <p class="small muted">
    {t('nsp.ready_note')}
  </p>
  {#if error}<p class="error">{error}</p>{/if}
</div>

<EqualizerPanel
  title={t('nsp.eq')}
  note={t('nsp.eq_note')}
  load={async () => $state.snapshot(draft.equalizer)}
  store={async (equalizer: EqualizerSettings) => {
    draft.equalizer = equalizer
    await save($state.snapshot(draft))
  }}
/>

<style>
  .speaker {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 8px 0 4px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .output {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 12px;
  }

  .output-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
</style>
