<script lang="ts">
  import { api } from './api'
  import { app, toast } from './state.svelte'

  $effect(() => {
    api
      .ai()
      .then((s) => (app.ai = s))
      .catch((e) => toast(e.message, true))
  })

  const mb = (bytes: number) => `${Math.round(bytes / 1_000_000)} MB`

  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await action()
      if (done) toast(done)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  async function remove() {
    if (!confirm('Remove the AI model? Songs it already heard are forgotten too until you download it again.')) return
    await attempt(() => api.removeAi(), 'AI model removed')
  }

  async function removeReader() {
    if (!confirm('Remove the AI reader model?')) return
    await attempt(() => api.removeReader(), 'AI reader removed')
  }

  const ai = $derived(app.ai)
  const reader = $derived(app.ai?.reader)
  const total = $derived(ai ? ai.heard + ai.waiting : 0)
</script>

<section class="card">
  <h2>AI listening (optional)</h2>
  <p class="small muted">
    An AI model listens to each song once, so searching by description ("80s hair metal", "music for cooking dinner")
    matches how songs actually sound, even ones with no genre tags. It runs on this computer; nothing is sent
    anywhere, and your music folders aren't changed.
  </p>
  {#if ai}
    {#if ai.error}<p class="small error">{ai.error}</p>{/if}
    {#if ai.downloading}
      <p class="small">Downloading the model: {mb(ai.downloading[0])} of {mb(ai.downloading[1])}</p>
      <progress max={ai.downloading[1]} value={ai.downloading[0]}></progress>
    {:else if !ai.installed}
      <p class="small muted">
        The model (LAION's CLAP for music, free to use) is a one-time {mb(ai.download_size)} download from Hugging
        Face. Listening takes about a second per song in the background.
      </p>
      <button class="primary" onclick={() => attempt(() => api.downloadAi())}>Download the model</button>
    {:else}
      <label class="switch big">
        <input
          type="checkbox"
          checked={ai.enabled}
          onchange={(e) => attempt(() => api.setAi(e.currentTarget.checked))}
        />
        Listen to songs with AI
      </label>
      {#if ai.enabled}
        {#if !ai.ready}
          <p class="small"><span class="spinner" aria-hidden="true"></span> Loading the model...</p>
        {:else if ai.waiting > 0}
          <p class="small">
            {#if ai.listening}<span class="spinner" aria-hidden="true"></span>{/if}
            Listened to {ai.heard} of {total} songs.
          </p>
          <progress max={total} value={ai.heard}></progress>
        {:else}
          <p class="small muted">Listened to all {total} songs.</p>
        {/if}
      {/if}
      <button class="small-button" onclick={remove}>Remove the model</button>
    {/if}
  {/if}
</section>

<section class="card">
  <h2>AI reader (optional)</h2>
  <p class="small muted">
    A small language model reads searches that DJINN's own phrase reader can't place, like "something for a first
    dance at a wedding, no rap", and turns them into genres, years, and moods. It runs on this computer; nothing is
    sent anywhere. Works best with AI listening on.
  </p>
  {#if reader}
    {#if reader.error}<p class="small error">{reader.error}</p>{/if}
    {#if reader.downloading}
      <p class="small">Downloading the model: {mb(reader.downloading[0])} of {mb(reader.downloading[1])}</p>
      <progress max={reader.downloading[1]} value={reader.downloading[0]}></progress>
    {:else if !reader.installed}
      <p class="small muted">
        The model (Qwen2.5 1.5B Instruct, free to use) is a one-time {mb(reader.download_size)} download from Hugging
        Face and needs about 1.5 GB of memory while on. Each new request takes a few seconds{reader.gpu_build
          ? ''
          : ' on the CPU'}; repeated ones are instant.
      </p>
      <button class="primary" onclick={() => attempt(() => api.downloadReader())}>Download the model</button>
    {:else}
      <label class="switch big">
        <input
          type="checkbox"
          checked={reader.enabled}
          onchange={(e) => attempt(() => api.setReader(e.currentTarget.checked))}
        />
        Read searches with AI
      </label>
      {#if reader.enabled && !reader.ready}
        <p class="small"><span class="spinner" aria-hidden="true"></span> Loading the model...</p>
      {/if}
      <button class="small-button" onclick={removeReader}>Remove the model</button>
    {/if}
  {/if}
</section>

<style>
  progress {
    width: 100%;
    accent-color: var(--accent);
  }

  .error {
    color: var(--bad);
  }

  .switch {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }

  .switch input {
    flex: none;
    width: 20px;
    min-height: 20px;
    accent-color: var(--accent);
  }

  .small-button {
    align-self: flex-start;
    min-height: 36px;
    padding: 0 12px;
    font-size: 0.8125rem;
  }
</style>
