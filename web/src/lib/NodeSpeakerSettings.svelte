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

  /** Most speakers one node plays to, the first included. */
  const MAX_OUTPUTS = 6

</script>

{#snippet speakerBlock(o: { device: string | null; delay_ms: number; trim: number }, title: string, remove?: () => void)}
  {@const others = [draft.device, ...(draft.outputs ?? []).map((x) => x.device)].filter((d) => d && d !== o.device)}
  <div class="output">
    <div class="output-head">
      <b>{title}</b>
      {#if remove}<button class="small-button danger" onclick={remove}>Remove</button>{/if}
    </div>
    <label>
      Speaker
      <select
        value={o.device ?? ''}
        onchange={(e) => {
          o.device = e.currentTarget.value || null
          changed(0)
        }}
      >
        <option value="">System default</option>
        {#each speakers.filter((s) => (!s.is_default || s.id === o.device) && !others.includes(s.id)) as speaker (speaker.id)}
          <option value={speaker.id}>{speaker.name}</option>
        {/each}
        {#if o.device && !speakers.some((s) => s.id === o.device)}
          <option value={o.device}>A speaker that isn't connected now</option>
        {/if}
      </select>
    </label>
    <label>
      Delay: {o.delay_ms} ms
      <input type="range" min="0" max="1000" step="5" bind:value={o.delay_ms} oninput={() => changed()} />
    </label>
    <label>
      Volume trim: {Math.round(o.trim * 100)}%
      <input type="range" min="0" max="1" step="0.01" bind:value={o.trim} oninput={() => changed()} />
    </label>
  </div>
{/snippet}

<div class="speaker">
  {@render speakerBlock(draft, (draft.outputs ?? []).length ? 'Speaker 1' : 'Speaker')}
  {#each draft.outputs ?? [] as output, i (i)}
    {@render speakerBlock(output, `Speaker ${i + 2}`, () => {
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
        Add another speaker
      </button>
    </div>
  {/if}
  <p class="small muted">
    Every speaker plays everything, each in step with the main DJINN. Delay holds one back, for speakers that lag
    (Bluetooth adds 100 to 300 ms); raise it until they sound like one. A speaker that isn't connected is skipped and
    the rest keep playing. Room timing nudges each one too.
  </p>
  <label>
    If the main DJINN can't be reached
    <select bind:value={draft.offline} onchange={() => changed(0)}>
      <option value="stop">Play what's loaded, then stop</option>
      <option value="cached">Play what's loaded, then keep playing cached songs</option>
    </select>
  </label>
  <NodeScreenSettings
    screen={draft.screen ?? 'tv'}
    visual={draft.visual ?? null}
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
