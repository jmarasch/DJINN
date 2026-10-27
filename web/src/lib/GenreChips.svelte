<script lang="ts">
  // Genre and style tags; each opens that genre's page.
  let { genres, onopen }: { genres: string[]; onopen?: () => void } = $props()
</script>

<div class="chips">
  {#each genres as genre (genre)}
    <a class="genre" href="#/genre/{encodeURIComponent(genre)}" onclick={() => onopen?.()}>{genre}</a>
  {/each}
</div>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .genre {
    padding: 2px 10px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 0.8125rem;
    text-decoration: none;
  }

  .genre:hover {
    color: var(--text);
  }
</style>
