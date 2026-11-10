<script lang="ts">
  // Beside a song's title: a hollow microphone when it has lyrics, a filled
  // one when they're timed (they follow the music). Nothing without any.
  import Icon from './Icon.svelte'
  import { t } from './i18n.svelte'

  let { lyrics, size = 14 }: { lyrics?: number; size?: number } = $props()

  const label = $derived(lyrics === 2 ? t('lyrics.timed_mark') : t('lyrics.words_mark'))
</script>

{#if lyrics}
  <span class="mark" class:timed={lyrics === 2} title={label} aria-label={label} role="img">
    <Icon name={lyrics === 2 ? 'lyrics' : 'lyrics-outline'} {size} />
  </span>
{/if}

<style>
  .mark {
    display: inline-flex;
    flex: none;
    vertical-align: -2px;
    margin-left: 4px;
    color: var(--muted);
  }

  .mark.timed {
    color: var(--accent);
  }
</style>
