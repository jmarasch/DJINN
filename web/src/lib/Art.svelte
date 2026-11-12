<script lang="ts">
  import { api } from './api'

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
    /** An artist's initial. Only marks the picture as an artist's, so a
     * missing one shows the artist placeholder instead of the record. */
    letter?: string | null
  } = $props()
  let failed = $state(false)
  // Small pictures get the 128 px thumbnail, medium ones 512 px; big ones
  // (Now Playing) the original, to stay sharp on high-density screens.
  const thumb = $derived(size <= 64 ? 128 : size < 300 ? 512 : undefined)
  // Placeholders: a glowing record for songs and albums, a silhouette for
  // artists; small ones for rows.
  const placeholder = $derived(
    `/images/missing-${letter != null ? 'artist' : 'album'}${size <= 64 ? '-small' : ''}.webp`,
  )
</script>

<div class="art" style:width="{size}px" style:height="{size}px" style:border-radius="{round}px">
  {#if hash && !failed}
    <img src={api.artUrl(hash, thumb)} alt="" loading="lazy" onerror={() => (failed = true)} />
  {:else}
    <img src={placeholder} alt="" />
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

  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
