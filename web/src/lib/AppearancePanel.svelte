<script lang="ts">
  // Admin > System > Background: the picture behind every page of the party
  // app. One of the built-in ones, or the admin's own.
  import { api } from './api'
  import { appearance, backgrounds } from './appearance.svelte'
  import BackgroundArt from './BackgroundArt.svelte'
  import { toast } from './state.svelte'

  let busy = $state(false)
  let strength = $state(appearance.strength)
  let file = $state<HTMLInputElement>()

  $effect(() => {
    strength = appearance.strength
  })

  const custom = $derived(appearance.background.startsWith('custom:'))

  function apply(found: { background: string; strength: number }) {
    appearance.background = found.background
    appearance.strength = found.strength
  }

  async function save(background: string, level = strength) {
    try {
      apply(await api.saveAppearance({ background, strength: level }))
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
  }

  async function chosen() {
    const picked = file?.files?.[0]
    if (!picked) return
    busy = true
    try {
      apply(await api.uploadBackground(picked))
      toast('Your picture is now the background')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not use that picture', true)
    } finally {
      busy = false
      if (file) file.value = ''
    }
  }

  async function removeCustom() {
    try {
      apply(await api.removeBackground())
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove it', true)
    }
  }
</script>

<section class="card">
  <h2>Background</h2>
  <p class="small muted">The picture behind every page of the party app, signed in or not. The TV and lyrics pages keep their own.</p>
  <div class="choices" role="radiogroup" aria-label="Background">
    {#each backgrounds as choice (choice.id)}
      <button
        class="choice"
        role="radio"
        aria-checked={appearance.background === choice.id}
        class:on={appearance.background === choice.id}
        onclick={() => save(choice.id)}
      >
        <span class="preview"><BackgroundArt name={choice.id} strength={90} /></span>
        <span>{choice.name}</span>
      </button>
    {/each}
    {#if custom}
      <button class="choice on" role="radio" aria-checked="true">
        <span class="preview"><BackgroundArt name={appearance.background} strength={90} /></span>
        <span>Your picture</span>
      </button>
    {/if}
  </div>
  <label class="strength">
    <span class="row">How strongly it shows <span class="muted">{strength}%</span></span>
    <input
      type="range"
      min="0"
      max="100"
      step="2"
      bind:value={strength}
      disabled={appearance.background === 'plain'}
      onchange={() => save(appearance.background, strength)}
    />
  </label>
  <div class="actions">
    <input bind:this={file} type="file" accept="image/*" hidden onchange={chosen} />
    <button disabled={busy} onclick={() => file?.click()}>
      {busy ? 'Uploading...' : custom ? 'Use another picture' : 'Use my own picture'}
    </button>
    {#if custom}<button onclick={removeCustom}>Remove my picture</button>{/if}
  </div>
  <p class="small muted">Pictures at least 800 by 450 pixels, up to 12 MB. Dark pictures read best.</p>
</section>

<style>
  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 10px;
  }

  .choice {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    padding: 6px 6px 8px;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--text);
    font-size: 0.875rem;
  }

  .choice.on {
    border-color: var(--edge-strong);
    box-shadow: var(--glow-strong);
    color: var(--accent);
  }

  .preview {
    position: relative;
    display: block;
    height: 72px;
    overflow: hidden;
    border-radius: 8px;
    background: #14111f;
  }

  .strength {
    max-width: 420px;
  }

  .row {
    display: flex;
    justify-content: space-between;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
