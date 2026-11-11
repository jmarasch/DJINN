<script lang="ts">
  // A node's speaker settings: which speaker, delay, trim, EQ, and what to
  // do when Main is gone. Used in Admin > Nodes and on the node's own page;
  // both save to Main, which keeps them.
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
        error = e instanceof Error ? e.message : 'Could not save'
      }
    }, delay)
  }

  const deviceId = $derived(draft.device ?? '')

</script>

<div class="speaker">
  <label>
    Speaker
    <select
      value={deviceId}
      onchange={(e) => {
        draft.device = e.currentTarget.value || null
        changed(0)
      }}
    >
      <option value="">System default</option>
      {#each speakers.filter((s) => !s.is_default || s.id === deviceId) as speaker (speaker.id)}
        <option value={speaker.id}>{speaker.name}</option>
      {/each}
      {#if draft.device && !speakers.some((s) => s.id === draft.device)}
        <option value={draft.device}>A speaker that isn't connected now</option>
      {/if}
    </select>
  </label>
  <label>
    Delay: {draft.delay_ms} ms
    <input type="range" min="0" max="1000" step="5" bind:value={draft.delay_ms} oninput={() => changed()} />
  </label>
  <p class="small muted">
    Holds this node's sound back, for speakers that lag (Bluetooth adds 100 to 300 ms). Raise it until the node and the
    main speakers sound like one.
  </p>
  <label>
    Volume trim: {Math.round(draft.trim * 100)}%
    <input type="range" min="0" max="1" step="0.01" bind:value={draft.trim} oninput={() => changed()} />
  </label>
  <label>
    If the main DJINN can't be reached
    <select bind:value={draft.offline} onchange={() => changed(0)}>
      <option value="stop">Play what's loaded, then stop</option>
      <option value="cached">Play what's loaded, then keep playing cached songs</option>
    </select>
  </label>
  <NodeScreenSettings
    screen={draft.screen ?? 'tv'}
    visual={draft.visual ?? 'bars'}
    onchange={(screen, visual) => {
      draft.screen = screen
      draft.visual = visual
      changed(0)
    }}
  />
  <p class="small muted">Only for a box installed with a screen. A screen already on changes within a few seconds.</p>
  <label class="check">
    <input
      type="checkbox"
      checked={!draft.free_run}
      onchange={(e) => {
        draft.free_run = !e.currentTarget.checked
        changed(0)
      }}
    />
    Keep in step with the main DJINN
  </label>
  <p class="small muted">
    On: this node eases its speed very slightly to stay with the main speakers, and only jumps if it gets more than a
    tenth of a second out. Off: it lines up when music starts, then plays straight through and slowly drifts.
  </p>
  <label>
    Songs to keep ready: {draft.cache_ahead ?? 12}
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
    Copies of what's playing and coming up, so a song is ready before it starts and the node can carry on if the main
    DJINN goes away. Past the requests, only a few songs are planned ahead.
  </p>
  {#if error}<p class="error">{error}</p>{/if}
</div>

<EqualizerPanel
  title="Equalizer for this node"
  note="Shapes only this node's speakers, on top of the party mix."
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

  label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
</style>
