<script lang="ts">
  // A song's lyrics. Timed ones follow the music: the line being sung is lit
  // and kept in the middle of the box. Plain ones are just the words.
  import type { Lyrics } from './types'

  let {
    lyrics,
    position,
    big = false,
    empty = 'No lyrics for this song.',
  }: {
    /** Undefined while loading, null when there are none. */
    lyrics: Lyrics | null | undefined
    /** Milliseconds into the song, now. */
    position: () => number
    /** The full-screen lyrics page. */
    big?: boolean
    empty?: string
  } = $props()

  let box = $state<HTMLElement>()
  let at = $state(-1)
  const lines = $derived(lyrics?.lines ?? null)

  // Which line is being sung: the last one started.
  $effect(() => {
    const timed = lines
    if (!timed) {
      at = -1
      return
    }
    let frame = 0
    const tick = () => {
      const ms = position()
      let lo = 0
      let hi = timed.length - 1
      let found = -1
      while (lo <= hi) {
        const mid = (lo + hi) >> 1
        if (timed[mid].ms <= ms) {
          found = mid
          lo = mid + 1
        } else hi = mid - 1
      }
      at = found
      frame = requestAnimationFrame(tick)
    }
    tick()
    return () => cancelAnimationFrame(frame)
  })

  // Keep the line being sung in the middle, without moving the page.
  $effect(() => {
    const index = at
    if (!box) return
    const line = box.querySelector<HTMLElement>(`[data-line="${Math.max(0, index)}"]`)
    if (!line) return
    const top = line.offsetTop - box.clientHeight / 2 + line.offsetHeight / 2
    const smooth = !matchMedia('(prefers-reduced-motion: reduce)').matches
    box.scrollTo({ top: Math.max(0, top), behavior: smooth ? 'smooth' : 'auto' })
  })
</script>

<div class="lyrics" class:big bind:this={box}>
  {#if lyrics === undefined}
    <p class="note">Loading lyrics...</p>
  {:else if lyrics === null}
    <p class="note">{empty}</p>
  {:else if lyrics.instrumental}
    <p class="note">Instrumental</p>
  {:else if lines}
    <div class="pad" aria-hidden="true"></div>
    {#each lines as line, i (i)}
      <p data-line={i} class:now={i === at} class:past={i < at}>{line.text || '♪'}</p>
    {/each}
    <div class="pad" aria-hidden="true"></div>
  {:else if lyrics.plain}
    <p class="plain">{lyrics.plain}</p>
  {/if}
</div>

<style>
  .lyrics {
    position: relative;
    max-height: 320px;
    overflow-y: auto;
    padding: 4px 2px;
    text-align: center;
    scrollbar-width: thin;
    mask-image: linear-gradient(transparent, #000 15%, #000 85%, transparent);
  }

  .lyrics p {
    margin: 0;
    padding: 5px 4px;
    color: var(--muted);
    font-size: 1.05rem;
    line-height: 1.35;
    transition:
      color 0.25s,
      transform 0.25s;
  }

  .lyrics p.now {
    color: var(--text);
    font-weight: 700;
    transform: scale(1.04);
  }

  .lyrics p.past {
    opacity: 0.7;
  }

  .pad {
    height: 40%;
  }

  .plain {
    white-space: pre-line;
    mask-image: none;
  }

  .note {
    padding: 24px 0;
  }

  .big {
    max-height: none;
    height: 100%;
    scrollbar-width: none;
  }

  .big::-webkit-scrollbar {
    display: none;
  }

  .big p {
    font-size: clamp(1.4rem, 3.4vw, 3rem);
    padding: 0.35em 0.5em;
  }

  .big p.now {
    transform: scale(1.06);
  }

  @media (prefers-reduced-motion: reduce) {
    .lyrics p {
      transition: none;
    }

    .lyrics p.now,
    .big p.now {
      transform: none;
    }
  }
</style>
