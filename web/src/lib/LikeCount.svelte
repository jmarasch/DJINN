<script lang="ts">
  // A heart with how many people favorited this: hollow, or filled when you
  // did. For people with favorites it's also the button; guests just see
  // the count.
  import { favorites, loadFavorites, toggleFavorite } from './favorites.svelte'
  import Icon from './Icon.svelte'
  import { hasPlaylists } from './state.svelte'
  import type { LikeKind } from './types'

  let { kind, id, name, likes = 0 }: { kind: LikeKind; id: number; name: string; likes?: number } = $props()

  /** Whether you had it favorited when the count was worked out. */
  let was = $state<boolean | null>(null)

  $effect(() => {
    void loadFavorites().then(() => {
      was ??= favorites[kind].has(id)
    })
  })

  const liked = $derived(favorites[kind].has(id))
  // The count came from the server before any tap here.
  const shown = $derived(Math.max(0, likes + (liked ? 1 : 0) - (was ? 1 : 0)))
</script>

{#if hasPlaylists()}
  <button
    class="like-count"
    class:liked
    aria-pressed={liked}
    aria-label={liked ? `Remove ${name} from favorites` : `Add ${name} to favorites`}
    title="{shown} {shown === 1 ? 'person' : 'people'} favorited this"
    onclick={(e) => {
      e.preventDefault()
      e.stopPropagation()
      void toggleFavorite(kind, id)
    }}
  >
    <Icon name={liked ? 'heart' : 'heart-outline'} size={18} />
    <span>{shown.toLocaleString()}</span>
  </button>
{:else}
  <span class="like-count" title="{shown} {shown === 1 ? 'person' : 'people'} favorited this">
    <Icon name="heart-outline" size={18} />
    <span>{shown.toLocaleString()}</span>
  </span>
{/if}

<style>
  .like-count {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-height: 40px;
    padding: 0 6px;
    background: transparent;
    color: var(--muted);
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
  }

  .like-count.liked {
    color: var(--accent);
  }

  .like-count.liked :global(svg) {
    filter: drop-shadow(0 0 5px rgb(180 140 255 / 0.7));
  }
</style>
