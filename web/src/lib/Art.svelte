<script lang="ts">
  import { api } from './api'
  import Icon from './Icon.svelte'

  let { hash, size = 48, round = 6 }: { hash: string | null; size?: number; round?: number } =
    $props()
  let failed = $state(false)
</script>

<div class="art" style:width="{size}px" style:height="{size}px" style:border-radius="{round}px">
  {#if hash && !failed}
    <img src={api.artUrl(hash)} alt="" loading="lazy" onerror={() => (failed = true)} />
  {:else}
    <Icon name="disc" size={Math.round(size * 0.5)} />
  {/if}
</div>

<style>
  .art {
    flex: none;
    display: grid;
    place-items: center;
    overflow: hidden;
    background: var(--surface-2);
    color: var(--muted);
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
