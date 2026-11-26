<script lang="ts">
  // Beside a song's title: a hollow microphone when it has lyrics, a filled
  // one when they're timed (they follow the music). Nothing without any. A
  // neon pink one when an AI made the words, a neon green one when a person's
  // words were timed by an AI. A red one, with no lyrics, when an AI's lyrics
  // for the song were removed as bad. A microphone in a "no" circle for an
  // instrumental: there are no words to sing.
  import Icon from './Icon.svelte'
  import { t } from './i18n.svelte'

  let {
    lyrics,
    ai = 0,
    failed = 0,
    instrumental = 0,
    size = 14,
  }: { lyrics?: number; ai?: number; failed?: number; instrumental?: number; size?: number } = $props()

  /** The microphone, drawn smaller inside the "no" circle. */
  const MIC =
    'M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3zm-1.2-9.1c0-.66.54-1.2 1.2-1.2s1.2.54 1.2 1.2v6.2c0 .66-.54 1.2-1.2 1.2s-1.2-.54-1.2-1.2zm6.5 6.1c0 3-2.54 5.1-5.3 5.1S6.7 14 6.7 11H5c0 3.41 2.72 6.23 6 6.72V21h2v-3.28c3.28-.48 6-3.3 6-6.72z'

  const label = $derived(
    ai === 1 ? t('lyrics.ai_made_mark') : ai === 2 ? t('lyrics.ai_timed_mark') : lyrics === 2 ? t('lyrics.timed_mark') : t('lyrics.words_mark'),
  )
</script>

{#if lyrics}
  <span class="mark" class:timed={lyrics === 2} class:ai-made={ai === 1} class:ai-timed={ai === 2} title={label} aria-label={label} role="img">
    <Icon name={lyrics === 2 ? 'lyrics' : 'lyrics-outline'} {size} />
  </span>
{:else if instrumental}
  <span class="mark" title={t('lyrics.instrumental_mark')} aria-label={t('lyrics.instrumental_mark')} role="img">
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <path d={MIC} fill="currentColor" transform="translate(5.65 5.9) scale(0.53)" />
      <circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" stroke-width="2" />
      <line x1="5" y1="5" x2="19" y2="19" stroke="currentColor" stroke-width="2" />
    </svg>
  </span>
{:else if failed}
  <span class="mark ai-failed" title={t('lyrics.ai_failed_mark')} aria-label={t('lyrics.ai_failed_mark')} role="img">
    <Icon name="lyrics-outline" {size} />
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

  .mark.ai-failed {
    color: var(--ai-failed);
    filter: drop-shadow(0 0 3px var(--ai-failed));
  }

  .mark.ai-timed {
    color: var(--ai-timed);
    filter: drop-shadow(0 0 3px var(--ai-timed));
  }
</style>
