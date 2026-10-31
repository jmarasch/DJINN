<script lang="ts">
  // Artist pictures that fade from one to the next, with the credit of the
  // one showing. Fills its box; the parent sets the size.
  import type { ArtistPicture } from './types'

  let {
    pictures,
    src,
    seconds = 8,
    credit = true,
    shown = $bindable(0),
  }: {
    pictures: ArtistPicture[]
    src: (picture: ArtistPicture) => string
    seconds?: number
    /** Show the credit inside; off when the parent shows it elsewhere. */
    credit?: boolean
    /** Which picture shows. */
    shown?: number
  } = $props()

  // A new artist starts from the first picture.
  $effect(() => {
    void pictures
    shown = 0
  })

  $effect(() => {
    if (pictures.length < 2) return
    const timer = setInterval(() => (shown = (shown + 1) % pictures.length), seconds * 1000)
    return () => clearInterval(timer)
  })

  const current = $derived(pictures[Math.min(shown, pictures.length - 1)])
</script>

{#if pictures.length}
  <div class="slideshow">
    {#each pictures as picture, i (picture.art_hash)}
      <img src={src(picture)} alt="" class:shown={i === shown} loading={i === 0 ? 'eager' : 'lazy'} />
    {/each}
    {#if credit && current}
      {#if current.credit_url}
        <a class="credit" href={current.credit_url} target="_blank" rel="noopener">{current.credit}{current.source === 'commons' ? ', Wikimedia Commons' : ''}</a>
      {:else}
        <span class="credit">{current.credit}</span>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .slideshow {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 25%;
    opacity: 0;
    transition: opacity 1.5s ease;
  }

  img.shown {
    opacity: 1;
  }

  .credit {
    position: absolute;
    right: 8px;
    bottom: 6px;
    left: 8px;
    overflow: hidden;
    color: rgb(255 255 255 / 0.85);
    font-size: 0.6875rem;
    text-align: right;
    text-overflow: ellipsis;
    text-shadow: 0 1px 3px rgb(0 0 0 / 0.8);
    white-space: nowrap;
  }

  @media (prefers-reduced-motion: reduce) {
    img {
      transition: none;
    }
  }
</style>
