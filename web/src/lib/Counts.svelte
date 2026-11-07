<script lang="ts">
  // How often something was played and how many people favorited it, as
  // " · 12 plays · ♥ 3" after a row's other details. Nothing when neither
  // is known or both are zero.
  import Icon from './Icon.svelte'
  import { t } from './i18n.svelte'

  let {
    plays,
    likes,
    lead = true,
  }: {
    plays?: number
    likes?: number
    /** False when nothing comes before it on the line, so no dot leads. */
    lead?: boolean
  } = $props()

  const dot = (first: boolean) => (first && !lead ? '' : ' · ')
</script>

{#if plays}<span class="meta">{dot(true)}{t('count.plays', { n: plays })}</span>{/if}
{#if likes}<span class="meta likes" title="{likes} {likes === 1 ? 'person' : 'people'} favorited this"
    >{dot(!plays)}<Icon name="heart" size={12} /><span class="sr">Favorited by</span>{' ' + likes.toLocaleString()}</span
  >{/if}

<style>
  .likes {
    white-space: nowrap;
  }

  .likes :global(svg) {
    vertical-align: -1px;
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
