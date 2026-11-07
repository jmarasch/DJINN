<script lang="ts">
  // The heart for a song, album or artist in the party app. Hidden from
  // guests, who have no favorites.
  import { favorites, loadFavorites, toggleFavorite } from './favorites.svelte'
  import Icon from './Icon.svelte'
  import { hasPlaylists } from './state.svelte'
  import type { LikeKind } from './types'

  let { kind, id, name, size = 20 }: { kind: LikeKind; id: number; name: string; size?: number } = $props()

  $effect(() => {
    void loadFavorites()
  })

  const liked = $derived(favorites[kind].has(id))
</script>

{#if hasPlaylists()}
  <button
    class="heart"
    class:liked
    aria-label={liked ? `Remove ${name} from favorites` : `Add ${name} to favorites`}
    aria-pressed={liked}
    onclick={(e) => {
      e.preventDefault()
      e.stopPropagation()
      void toggleFavorite(kind, id)
    }}
  >
    <Icon name={liked ? 'heart' : 'heart-outline'} {size} />
  </button>
{/if}

<style>
  .heart {
    flex: none;
    width: 40px;
    min-height: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .heart.liked {
    color: var(--accent);
  }
</style>
