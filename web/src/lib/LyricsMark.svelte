<script lang="ts">
  // Beside a song's title: a hollow microphone when it has lyrics, a filled
  // one when they're timed (they follow the music). Nothing without any. A
  // neon pink one when an AI made the words, a neon green one when a person's
  // words were timed by an AI.
  import Icon from './Icon.svelte'
  import { t } from './i18n.svelte'

  let { lyrics, ai = 0, size = 14 }: { lyrics?: number; ai?: number; size?: number } = $props()

  const label = $derived(
    ai === 1 ? t('lyrics.ai_made_mark') : ai === 2 ? t('lyrics.ai_timed_mark') : lyrics === 2 ? t('lyrics.timed_mark') : t('lyrics.words_mark'),
  )
</script>

{#if lyrics}
  <span class="mark" class:timed={lyrics === 2} class:ai-made={ai === 1} class:ai-timed={ai === 2} title={label} aria-label={label} role="img">
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

  .mark.ai-made {
    color: var(--ai-made);
    filter: drop-shadow(0 0 3px var(--ai-made));
  }

  .mark.ai-timed {
    color: var(--ai-timed);
    filter: drop-shadow(0 0 3px var(--ai-timed));
  }
</style>
