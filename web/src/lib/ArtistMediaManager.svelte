<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from './api'
  import Sheet from './Sheet.svelte'
  import { toast } from './state.svelte'
  import type { ArtistPicture, UnsplashChoice } from './types'

  let {
    id,
    name,
    onclose,
    onchange,
  }: {
    id: number
    name: string
    onclose: () => void
    onchange: (pictures: ArtistPicture[]) => void
  } = $props()

  let pictures = $state<ArtistPicture[]>([])
  let credit = $state('')
  let creditUrl = $state('')
  let file = $state<HTMLInputElement | undefined>()
  let busy = $state(false)
  let loading = $state(true)
  let remoteAllowed = $state(false)
  let unsplash = $state<UnsplashChoice[]>([])
  let searching = $state(false)

  onMount(async () => {
    try {
      const [list, settings] = await Promise.all([api.manageArtistPictures(id), api.settings()])
      pictures = list
      remoteAllowed = settings.picture_settings.allow_remote
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load pictures', true)
    } finally {
      loading = false
    }
  })

  const sources = [
    { name: 'Wikimedia Commons', url: 'https://commons.wikimedia.org/wiki/Special:MediaSearch?type=image' },
    { name: 'fanart.tv', url: 'https://fanart.tv/music-fanart/' },
    { name: 'TheAudioDB', url: 'https://www.theaudiodb.com/' },
    { name: 'Getty Images', url: 'https://www.gettyimages.com/photos/famous-musicians' },
    { name: 'iStock', url: 'https://www.istockphoto.com/photos/music-artist' },
    { name: 'Unsplash', url: 'https://unsplash.com/s/photos/music-artist' },
    { name: "Reelin' in the Years Archive", url: 'https://photos.reelinintheyears.com/' },
    { name: 'Photos.com', url: 'https://photos.com/collections/music+lovers' },
    { name: 'Rolling Stone photo collections', url: 'https://www.rollingstone.com/product-recommendations/lifestyle/best-music-photography-prints-art-1052198/' },
    { name: 'Pinterest ideas', url: 'https://www.pinterest.com/ideas/iconic-music-photos/929451450092/' },
    { name: 'Flickr Creative Commons', url: 'https://www.flickr.com/creativecommons/' },
    { name: 'Internet Archive', url: 'https://archive.org/details/image?query=musician' },
  ]

  function changed(list: ArtistPicture[]) {
    pictures = list
    onchange(list)
  }

  async function upload() {
    const chosen = file?.files?.[0]
    if (!chosen || !credit.trim()) {
      toast('Choose an image and add its photo credit', true)
      return
    }
    busy = true
    try {
      changed(await api.uploadArtistPicture(id, chosen, credit.trim(), creditUrl.trim()))
      credit = ''
      creditUrl = ''
      if (file) file.value = ''
      toast('Artist picture added')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not add picture', true)
    } finally {
      busy = false
    }
  }

  async function remove(picture: ArtistPicture) {
    if (!confirm(`Remove this picture from ${name}? A host-uploaded file cannot be recovered here.`)) return
    busy = true
    try {
      changed(await api.removeArtistPicture(id, picture.art_hash))
      toast('Picture removed')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove picture', true)
    } finally {
      busy = false
    }
  }

  async function move(index: number, direction: -1 | 1) {
    const order = pictures.map((picture) => picture.art_hash)
    const next = index + direction
    ;[order[index], order[next]] = [order[next], order[index]]
    busy = true
    try {
      changed(await api.orderArtistPictures(id, order))
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change picture order', true)
    } finally {
      busy = false
    }
  }

  async function recheck() {
    busy = true
    try {
      changed(await api.recheckArtistPictures(id))
      toast('Picture sources checked')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not check Commons', true)
    } finally {
      busy = false
    }
  }

  async function findUnsplash() {
    searching = true
    try {
      unsplash = await api.searchUnsplash(id)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not search Unsplash', true)
    } finally {
      searching = false
    }
  }

  async function selectUnsplash(photoId: string) {
    busy = true
    try {
      changed(await api.addUnsplashPicture(id, photoId))
      toast('Remote picture added')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not add picture', true)
    } finally {
      busy = false
    }
  }
</script>

<Sheet title="Manage artist pictures" {onclose}>
  <p class="small muted">Extra pictures for {name}. Move them to choose the slideshow order. Removing an unused picture also frees its space in DJINN's art cache.</p>
  {#if loading}
    <p class="muted">Loading...</p>
  {:else}
    <p class="small muted">{pictures.length} {pictures.length === 1 ? 'picture' : 'pictures'}</p>
    {#each pictures as picture, i (picture.art_hash)}
      <div class="picture">
        {#if picture.remote_url && !remoteAllowed}
          <span class="remote-off">Remote display off</span>
        {:else}
          <img src={picture.remote_url ?? api.artUrl(picture.art_hash, 128)} alt="" />
        {/if}
        <div class="details">
          <b>{{ manual: 'Added by host', commons: 'Wikimedia Commons', fanart: 'fanart.tv', audiodb: 'TheAudioDB', unsplash: 'Unsplash' }[picture.source]}</b>
          <span class="small muted">{picture.credit}</span>
          {#if picture.credit_url}<a class="small" href={picture.credit_url} target="_blank" rel="noopener noreferrer">Source</a>{/if}
        </div>
        <div class="picture-actions">
          <button disabled={busy || i === 0} aria-label="Move picture up" onclick={() => move(i, -1)}>↑</button>
          <button disabled={busy || i === pictures.length - 1} aria-label="Move picture down" onclick={() => move(i, 1)}>↓</button>
          <button disabled={busy} aria-label="Remove picture" onclick={() => remove(picture)}>×</button>
        </div>
      </div>
    {/each}
    <button disabled={busy} onclick={recheck}>Find automatic pictures again</button>
  {/if}

  <h3>Add a picture</h3>
  <p class="small muted">Browse these sources, then upload only a picture you have the rights to use. Stock and archive sites may require a license. Unsplash API images need hotlinking, so they are not downloaded automatically.</p>
  <div class="source-links">
    {#each sources as source}<a href={source.url} target="_blank" rel="noopener noreferrer">{source.name}</a>{/each}
  </div>
  <label class="field">Image file<input bind:this={file} type="file" accept="image/jpeg,image/png,image/webp" /></label>
  <label class="field">Photo credit<input bind:value={credit} maxlength="200" placeholder="Photographer / license" /></label>
  <label class="field">Source or license link (optional)<input bind:value={creditUrl} type="url" maxlength="500" placeholder="https://..." /></label>
  <button class="primary" disabled={busy} onclick={upload}>Add picture</button>
  <h3>Unsplash photos</h3>
  <p class="small muted">Search for {name} and choose a matching photo. These images load directly from Unsplash and show the photographer's credit. Enable remote images and add an Unsplash access key in Library settings first.</p>
  <button disabled={busy || searching} onclick={findUnsplash}>{searching ? 'Searching...' : 'Search Unsplash'}</button>
  {#if unsplash.length}
    <div class="unsplash-grid">
      {#each unsplash as photo (photo.id)}
        <div class="unsplash-choice">
          <img src={photo.thumbnail} alt="" loading="lazy" />
          <a href={photo.credit_url} target="_blank" rel="noopener noreferrer">{photo.credit}</a>
          <button disabled={busy} onclick={() => selectUnsplash(photo.id)}>Use photo</button>
        </div>
      {/each}
    </div>
  {/if}
</Sheet>

<style>
  h3 { margin: 22px 0 4px; }
  .picture { display: flex; align-items: center; gap: 10px; padding: 8px 0; border-bottom: 1px solid var(--line); }
  .picture img { width: 64px; height: 64px; flex: none; border-radius: 8px; object-fit: cover; }
  .remote-off { display: grid; place-items: center; width: 64px; height: 64px; flex: none; border-radius: 8px; background: var(--surface); font-size: 0.7rem; text-align: center; }
  .details { display: flex; flex: 1; flex-direction: column; min-width: 0; overflow-wrap: anywhere; }
  .picture-actions { display: flex; flex-direction: column; }
  .picture-actions button { min-width: 36px; min-height: 28px; padding: 0; }
  .source-links { display: flex; flex-wrap: wrap; gap: 6px 12px; margin: 10px 0 18px; }
  .source-links a { font-size: 0.875rem; }
  .field { display: grid; gap: 4px; margin: 10px 0; }
  .field input { width: 100%; }
  .unsplash-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(130px, 1fr)); gap: 12px; margin-top: 12px; }
  .unsplash-choice { display: grid; gap: 5px; font-size: 0.8rem; }
  .unsplash-choice img { width: 100%; aspect-ratio: 4/3; object-fit: cover; border-radius: 8px; }
</style>
