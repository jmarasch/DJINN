<script lang="ts">
  // The heart for a song, album or artist.
  import Icon from '../lib/Icon.svelte'
  import type { LikeKind } from '../lib/types'
  import { likes, toggleLike } from './player.svelte'

  let { kind, id, name, size = 20 }: { kind: LikeKind; id: number; name: string; size?: number } = $props()

  const liked = $derived(likes[kind].has(id))
</script>

<button
  class="like"
  class:liked
  aria-label={liked ? `Unlike ${name}` : `Like ${name}`}
  aria-pressed={liked}
  onclick={(e) => {
    e.preventDefault()
    e.stopPropagation()
    toggleLike(kind, id)
  }}
>
  <Icon name={liked ? 'heart' : 'heart-outline'} {size} />
</button>

<style>
  .like {
    flex: none;
    width: 40px;
    min-height: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .like.liked {
    color: var(--accent);
  }
</style>
