<script lang="ts">
  // Admin > Library: what a library check does beyond reading folders and
  // tags. Each extra is off until switched on; switches save right away,
  // text fields with their own Save.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { clock, heldText } from './background'
  import IdentifyPanel from './IdentifyPanel.svelte'
  import { app, toast } from './state.svelte'
  import type { ArtCacheStatus, Settings } from './types'

  let { settings = $bindable() }: { settings: Settings } = $props()
  let cache = $state<ArtCacheStatus | null>(null)

  onMount(() => { void loadCache() })

  async function loadCache() {
    cache = await api.artCacheStatus().catch(() => null)
  }

  async function save(done?: string) {
    try {
      settings = await api.saveSettings(settings)
      void loadCache()
      if (done) toast(done)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
      // Show what's really saved.
      settings = await api.settings()
    }
  }

  async function lookUp(force: boolean) {
    try {
      const { status } = await api.lookUpArtistInfo(force)
      toast(
        status === 'started'
          ? force
            ? 'Looking up every artist again'
            : 'Looking up new artists'
          : status === 'queued'
            ? 'A lookup is running. Every artist gets rechecked right after it.'
            : 'Already looking up artists. Results show up when it finishes.',
      )
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the lookup', true)
    }
  }

  const hours = Array.from({ length: 24 }, (_, h) => h)
  const n = (x: number) => x.toLocaleString()
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })

  const analysis = $derived(app.loudness)
  const lookups = $derived(settings.online_artist_info || settings.musicbrainz)
  const mb = (bytes: number) => (bytes / (1024 * 1024)).toFixed(1)
</script>

<h2 class="group">Extras</h2>
<p class="small muted intro">
  A library check reads your music folders and the songs' tags. Everything below adds more, and each is off until you
  switch it on: some take a lot of this computer's time, some ask services online.
</p>

<section class="card">
  <label class="switch big">
    <input type="checkbox" bind:checked={settings.sound_analysis} onchange={() => save()} />
    <b>Sound analysis</b>
  </label>
  <p class="small muted">
    Listens to every song once, on this computer, for "more like this", similar artists, radio by sound, smart play, and
    even volume for songs without ReplayGain tags. It's the heaviest job here: on a modest computer about a song a
    second, using up to half the processor, so a big library takes hours the first time.
  </p>
  {#if settings.sound_analysis && analysis}
    {@const total = analysis.analyzed + analysis.waiting}
    {#if analysis.held && analysis.waiting > 0}
      <p class="small">{heldText(analysis.held)}: {n(analysis.analyzed)} of {n(total)} songs done.</p>
    {:else if analysis.waiting > 0}
      <p class="small">
        {#if analysis.running}<span class="spinner" aria-hidden="true"></span>{/if}
        {n(analysis.analyzed)} of {n(total)} songs done.
      </p>
    {:else if total}
      <p class="small muted">All {n(total)} songs analyzed.</p>
    {/if}
  {/if}
</section>

<section class="card">
  <h3>When heavy work runs</h3>
  <p class="small muted">For sound analysis and AI listening. They pause between songs, so music never waits for them.</p>
  <div class="choices">
    <label class="switch">
      <input type="radio" value="any" bind:group={settings.background.when} onchange={() => save()} />
      Any time
    </label>
    <label class="switch">
      <input type="radio" value="idle" bind:group={settings.background.when} onchange={() => save()} />
      Only while no music is playing
    </label>
    <label class="switch">
      <input type="radio" value="hours" bind:group={settings.background.when} onchange={() => save()} />
      <span class="hours">
        Only from
        <select
          bind:value={settings.background.from}
          onchange={() => save()}
          disabled={settings.background.when !== 'hours'}
          aria-label="From"
        >
          {#each hours as h (h)}<option value={h}>{clock(h)}</option>{/each}
        </select>
        until
        <select
          bind:value={settings.background.until}
          onchange={() => save()}
          disabled={settings.background.when !== 'hours'}
          aria-label="Until"
        >
          {#each hours as h (h)}<option value={h}>{clock(h)}</option>{/each}
        </select>
      </span>
    </label>
  </div>
  <p class="small muted">Hours are this computer's time.</p>
</section>

<section class="card">
  <label class="switch big">
    <input type="checkbox" bind:checked={settings.online_artist_info} onchange={() => save()} />
    <b>Artist photos and bios</b>
  </label>
  <p class="small muted">
    Sends artist names to Deezer, Wikipedia and Wikimedia Commons, after each library check that finds new artists.
    Pictures in your own folders always win.
  </p>
  <label class="switch sub">
    <input
      type="checkbox"
      bind:checked={settings.artist_pictures}
      disabled={!settings.online_artist_info}
      onchange={() => save()}
    />
    <span>
      More pictures, as a slideshow on artist pages and on the TV page while their songs play
      <span class="small muted">
        (from the sources you choose below, with credits. Commons and fanart.tv use MusicBrainz to find the right artist.
        Each artist's pictures are fetched the first time they're shown.)
      </span>
    </span>
  </label>
  <div class="picture-limits">
    <h3>Picture download limits</h3>
    <p class="small muted">These apply to extra artist pictures. Downloaded images are resized before they enter the cache. The cache limit also covers album art and thumbnails; existing files stay when you lower it.</p>
    <div class="limit-grid">
      <label>Pictures per artist<input type="number" min="1" max="20" bind:value={settings.picture_settings.count} /></label>
      <label>MB per artist (0 = no cap)<input type="number" min="0" max="200" bind:value={settings.picture_settings.per_artist_mb} /></label>
      <label>Art cache maximum MB (0 = no cap)<input type="number" min="0" max="100000" bind:value={settings.picture_settings.cache_mb} /></label>
      <label>Maximum image edge, pixels<input type="number" min="512" max="4096" bind:value={settings.picture_settings.max_edge} /></label>
      <label>Minimum source width, pixels<input type="number" min="0" max="4096" bind:value={settings.picture_settings.min_width} /></label>
      <label>Minimum source height, pixels<input type="number" min="0" max="4096" bind:value={settings.picture_settings.min_height} /></label>
    </div>
    <button onclick={() => save('Picture limits saved')}>Save picture limits</button>
    {#if cache}
      <p class="small muted">Art cache: {mb(cache.used_bytes)} MB used{cache.max_bytes ? ` of ${mb(cache.max_bytes)} MB` : ' (no limit)'}.</p>
      {#if cache.max_bytes}
        <progress max={cache.max_bytes} value={Math.min(cache.used_bytes, cache.max_bytes)} aria-label="Art cache used"></progress>
      {/if}
    {/if}
  </div>
  <div class="picture-limits">
    <h3>Artist picture sources</h3>
    <p class="small muted">Choose automatic sources here. MusicBrainz identifies artists and albums when enabled above. Source changes are applied as each artist is checked again.</p>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.deezer} /> Deezer (main artist portrait, local copy, no key)</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.wikipedia} /> Wikipedia (artist bios, no key)</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.commons} /> Wikimedia Commons (local copy, no key)</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.fanart} /> fanart.tv (local copy, MusicBrainz match)</label>
    <label class="field">fanart.tv API key<input type="password" bind:value={settings.picture_settings.fanart_key} autocomplete="off" placeholder="Your API key" /></label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.audiodb} /> TheAudioDB (local copy)</label>
    <label class="field">TheAudioDB API key<input type="password" bind:value={settings.picture_settings.audiodb_key} autocomplete="off" placeholder="123 is the free key" /></label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.allow_remote} /> Allow remote image display (visitors load images from the provider)</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.unsplash} disabled={!settings.picture_settings.allow_remote} /> Unsplash (browse and select, remote display only)</label>
    <label class="field">Unsplash access key<input type="password" bind:value={settings.picture_settings.unsplash_key} autocomplete="off" placeholder="Your Unsplash application access key" /></label>
    <button onclick={() => save('Picture sources saved')}>Save picture sources</button>
    <h3>Browse and import</h3>
    <p class="small muted">Open these libraries to find artist photos. For licensed images, use Manage pictures on an artist page to upload a copy you are allowed to use, with its credit.</p>
    <div class="source-links">
      <a href="https://commons.wikimedia.org/wiki/Special:MediaSearch?type=image" target="_blank" rel="noopener noreferrer">Wikimedia Commons</a>
      <a href="https://fanart.tv/music-fanart/" target="_blank" rel="noopener noreferrer">fanart.tv</a>
      <a href="https://www.theaudiodb.com/" target="_blank" rel="noopener noreferrer">TheAudioDB</a>
      <a href="https://www.gettyimages.com/photos/famous-musicians" target="_blank" rel="noopener noreferrer">Getty Images</a>
      <a href="https://www.istockphoto.com/photos/music-artist" target="_blank" rel="noopener noreferrer">iStock</a>
      <a href="https://unsplash.com/s/photos/music-artist" target="_blank" rel="noopener noreferrer">Unsplash</a>
      <a href="https://photos.reelinintheyears.com/" target="_blank" rel="noopener noreferrer">Reelin' in the Years</a>
      <a href="https://photos.com/collections/music+lovers" target="_blank" rel="noopener noreferrer">Photos.com</a>
      <a href="https://www.rollingstone.com/product-recommendations/lifestyle/best-music-photography-prints-art-1052198/" target="_blank" rel="noopener noreferrer">Rolling Stone photo collections</a>
      <a href="https://www.pinterest.com/ideas/iconic-music-photos/929451450092/" target="_blank" rel="noopener noreferrer">Pinterest ideas</a>
      <a href="https://www.flickr.com/creativecommons/" target="_blank" rel="noopener noreferrer">Flickr Creative Commons</a>
      <a href="https://archive.org/details/image?query=musician" target="_blank" rel="noopener noreferrer">Internet Archive</a>
    </div>
  </div>
</section>

<section class="card">
  <label class="switch big">
    <input
      type="checkbox"
      bind:checked={settings.musicbrainz}
      disabled={!settings.musicbrainz && !settings.musicbrainz_contact.trim()}
      onchange={() => save()}
    />
    <b>MusicBrainz</b>
  </label>
  <p class="small muted">
    Album covers, album and artist genres, and exact artist matches (better photos and bios), from MusicBrainz and the
    Cover Art Archive. Sends artist and album names. MusicBrainz needs no account, but asks apps to say who to contact if
    something goes wrong; it's sent with each request.
  </p>
  <form
    class="inline"
    onsubmit={(e) => {
      e.preventDefault()
      save('Saved')
    }}
  >
    <label>
      Contact (email or website)
      <input
        type="text"
        bind:value={settings.musicbrainz_contact}
        placeholder="you@example.com"
        maxlength="200"
        autocapitalize="none"
      />
    </label>
    <button>Save</button>
  </form>
</section>

{#if lookups}
  <section class="card">
    <h3>Looking up artists</h3>
    {#if app.lookup.running}
      <p class="small"><span class="spinner" aria-hidden="true"></span> Looking up artists... New pictures show up when it's done.</p>
    {:else if app.lookup.last && app.lookup.finished_at}
      <p class="small muted">
        Last lookup {when(app.lookup.finished_at)}: {app.lookup.last.checked} artists checked{settings.online_artist_info
          ? `, ${app.lookup.last.photos} photos, ${app.lookup.last.bios} bios`
          : ''}{settings.musicbrainz
          ? `, ${app.lookup.last.covers} album covers, ${app.lookup.last.genres} with genres`
          : ''} found.
      </p>
    {/if}
    <div class="row-buttons">
      <button disabled={app.lookup.running} onclick={() => lookUp(false)}>Look up new artists</button>
      <button onclick={() => lookUp(true)}>Recheck all</button>
    </div>
  </section>
{/if}

<section class="card">
  <h3>Identify songs by sound</h3>
  <p class="small muted">
    Needs an AcoustID key and the MusicBrainz contact above. Song fingerprints (not the audio) go to AcoustID. It only
    runs when you start it, below.
  </p>
  <form
    class="inline"
    onsubmit={(e) => {
      e.preventDefault()
      save('Saved')
    }}
  >
    <label>
      AcoustID key
      <input
        type="text"
        bind:value={settings.acoustid_key}
        placeholder="Leave empty to skip"
        maxlength="64"
        autocapitalize="none"
        spellcheck="false"
      />
    </label>
    <button>Save</button>
  </form>
  <p class="small muted">
    Free for non-commercial use: register an application at
    <a href="https://acoustid.org/new-application" target="_blank" rel="noopener noreferrer">acoustid.org/new-application</a>
    and paste its key.
  </p>
</section>
<IdentifyPanel />

<style>
  .picture-limits {
    margin-top: 16px;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }

  .limit-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 10px;
    margin: 12px 0;
  }

  .limit-grid label {
    display: grid;
    gap: 4px;
    font-size: 0.875rem;
  }

  .limit-grid input {
    width: 100%;
  }
  .field { display: grid; gap: 4px; max-width: 440px; margin: 10px 0; }
  .field input { width: 100%; }
  .source-links { display: flex; flex-wrap: wrap; gap: 6px 12px; margin: 10px 0; }

  progress {
    width: 100%;
  }
  .group {
    margin: 24px 0 4px;
  }

  .intro {
    margin: 0 0 12px;
  }

  h3 {
    margin: 0 0 6px;
  }

  .sub {
    margin: 4px 0 0 28px;
  }

  .choices {
    display: grid;
    gap: 8px;
    margin: 8px 0;
  }

  .hours {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 8px;
  }

  .inline label {
    display: grid;
    gap: 4px;
    flex: 1 1 220px;
  }

  .row-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
