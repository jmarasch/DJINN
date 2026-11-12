<script lang="ts">
  // A screen's big title between two small waveforms, with an optional
  // line under it, as in the party app's Library, Favorites and Queue.
  let {
    title,
    subtitle = '',
    align = 'center',
  }: { title: string; subtitle?: string; align?: 'center' | 'left' } = $props()
  // Bar heights for each side's waveform, mirrored on the right.
  const left = [3, 6, 10, 15, 9, 13, 6, 4]
  const right = [...left].reverse()
</script>

{#snippet wave(heights: number[])}
  <svg width="44" height="22" viewBox="0 0 44 22" aria-hidden="true" class="wave">
    {#each heights as h, i (i)}
      <rect x={i * 5.5 + 1} y={11 - h / 2 - 1} width="2.4" height={h + 2} rx="1.2" />
    {/each}
  </svg>
{/snippet}

<header class="page-title" class:left={align === 'left'}>
  <div class="line">
    {@render wave(left)}
    <h1>{title}</h1>
    {@render wave(right)}
  </div>
  {#if subtitle}<p>{subtitle}</p>{/if}
</header>

<style>
  .page-title {
    margin: 8px 0 16px;
    text-align: center;
  }

  .left {
    text-align: left;
  }

  .left .line {
    justify-content: flex-start;
  }

  .line {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
  }

  h1 {
    margin: 0;
    font-size: 2.25rem;
    text-shadow: 0 0 18px rgb(180 140 255 / 0.35);
  }

  .wave {
    flex: none;
    fill: var(--accent);
    filter: drop-shadow(0 0 4px rgb(180 140 255 / 0.6));
  }

  p {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: 0.75rem;
    letter-spacing: 0.22em;
    text-transform: uppercase;
  }

  @media (max-width: 380px) {
    h1 {
      font-size: 1.875rem;
    }

    .wave {
      width: 30px;
    }
  }
</style>
