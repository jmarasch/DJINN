<script lang="ts">
  // Every genre in the library as a tag with its number of songs; each opens
  // that genre's page.
  import type { GenreMap } from './types'

  let { map }: { map: GenreMap } = $props()
</script>

<div class="tags">
  {#each map.genres as genre (genre.name)}
    <a class="tag" href="#/genre/{encodeURIComponent(genre.name)}">
      {genre.name}
      <span class="count">{genre.songs}</span>
    </a>
  {/each}
</div>

<style>
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding: 4px 0 16px;
  }

  .tag {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 0 14px;
    border-radius: 999px;
    background: var(--surface);
    color: var(--text);
    text-decoration: none;
  }

  .tag:hover {
    background: var(--surface-2);
  }

  .count {
    color: var(--muted);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }
</style>
