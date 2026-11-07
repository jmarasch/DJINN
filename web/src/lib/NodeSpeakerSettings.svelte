<script lang="ts">
  // A node's speaker settings: which speaker, delay, trim, EQ, and what to
  // do when Main is gone. Used in Admin > Nodes and on the node's own page;
  // both save to Main, which keeps them.
  import EqualizerPanel from './EqualizerPanel.svelte'
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
</style>
