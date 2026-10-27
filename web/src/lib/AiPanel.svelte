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

  const ai = $derived(app.ai)
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

  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    margin-right: 6px;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 50%;
    vertical-align: -1px;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
