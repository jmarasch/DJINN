<script lang="ts">
  // Genre and style tags; each opens that genre's page. With `onremove`,
  // each also gets a button to take it off.
  let {
    genres,
    onopen,
    onremove,
  }: { genres: string[]; onopen?: () => void; onremove?: (genre: string) => void } = $props()
</script>

<div class="chips">
  {#each genres as genre (genre)}
    <span class="genre">
      <a href="#/genre/{encodeURIComponent(genre)}" onclick={() => onopen?.()}>{genre}</a>
      {#if onremove}
        <button
          class="remove"
          aria-label="Remove {genre} from this song"
          title="Doesn't sound like {genre}"
          onclick={() => onremove(genre)}>×</button
        >
      {/if}
    </span>
  {/each}
</div>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .genre {
    display: inline-flex;
    align-items: center;
    border-radius: 999px;
    background: var(--surface-2);
    font-size: 0.8125rem;
  }

  .genre a {
    padding: 2px 10px;
    color: var(--muted);
    text-decoration: none;
  }

  .genre a:hover {
    color: var(--text);
  }

  .remove {
    min-width: 0;
    min-height: 0;
    margin-left: -6px;
    padding: 2px 9px 2px 4px;
    border: 0;
    border-radius: 999px;
    background: none;
    color: var(--muted);
    font-size: 0.9375rem;
    line-height: 1;
  }

  .remove:hover {
    color: var(--text);
  }
</style>
