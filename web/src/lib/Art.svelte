<script lang="ts">
  import { api } from './api'
  import Icon from './Icon.svelte'

  let {
    hash,
    size = 48,
    round = 6,
    letter = null,
  }: {
    hash: string | null
    size?: number
    /** Corner radius in px; use a large value for a circle. */
    round?: number
    /** Shown instead of the disc icon when there is no image. */
    letter?: string | null
  } = $props()
  let failed = $state(false)
</script>

<div class="art" style:width="{size}px" style:height="{size}px" style:border-radius="{round}px">
  {#if hash && !failed}
    <img src={api.artUrl(hash)} alt="" loading="lazy" onerror={() => (failed = true)} />
  {:else if letter}
    <span class="letter" style:font-size="{Math.round(size * 0.42)}px">{letter}</span>
  {:else}
    <Icon name="disc" size={Math.round(size * 0.5)} />
  {/if}
</div>

<style>
  .art {
    flex: none;
    display: grid;
    place-items: center;
    overflow: hidden;
    background: var(--surface-2);
    color: var(--muted);
  }

  .letter {
    color: var(--accent);
    font-weight: 600;
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
