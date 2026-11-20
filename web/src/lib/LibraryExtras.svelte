<script lang="ts">
  import SecretInput from './SecretInput.svelte'
  // Admin > Library: what a library check does beyond reading folders and
  // tags. Each extra is off until switched on; switches save right away,
  // text fields with their own Save.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { clock, heldText } from './background'
  import IdentifyPanel from './IdentifyPanel.svelte'
  import LyricsPanel from './LyricsPanel.svelte'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
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
      toast(e instanceof Error ? e.message : t('common.failed'), true)
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
            ? t('libx.lookup_all')
            : t('libx.lookup_new')
          : status === 'queued'
            ? t('libx.lookup_queued')
            : t('libx.lookup_running'),
      )
    } catch (e) {
      toast(e instanceof Error ? e.message : t('libx.lookup_failed'), true)
    }
  }

  const hours = Array.from({ length: 24 }, (_, h) => h)
  const n = (x: number) => x.toLocaleString(locale.code)
  const when = (ms: number) =>
    new Date(ms).toLocaleString(locale.code, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })

  const analysis = $derived(app.loudness)
  const lookups = $derived(settings.online_artist_info || settings.musicbrainz)
  const mb = (bytes: number) => (bytes / (1024 * 1024)).toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })

  let spotifyId = $state('')
  let spotifySecret = $state('')
  let spotifySaved = $state<{ client_id: string | null; has_secret: boolean } | null>(null)
  let spotifyBusy = $state(false)

  async function loadSpotify() {
    try {
      spotifySaved = await api.spotify()
      spotifyId = spotifySaved.client_id ?? ''
    } catch {
      spotifySaved = null
    }
  }

  async function saveSpotify(remove = false) {
    spotifyBusy = true
    try {
      await api.setSpotify(remove ? '' : spotifyId, remove ? '' : spotifySecret)
      spotifySecret = ''
      toast(remove ? t('libx.spotify_removed') : t('libx.spotify_ok'))
      await loadSpotify()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed'), true)
    } finally {
      spotifyBusy = false
    }
  }

  let hasYoutubeKey = $state(false)
  let youtubeKey = $state('')

  onMount(() => {
    void loadSpotify()
    api
      .youtubeKey()
      .then((status) => (hasYoutubeKey = status.has_key))
      .catch(() => {})
  })

  async function saveYoutubeKey(key: string) {
    try {
      await api.setYoutubeKey(key)
      hasYoutubeKey = !!key.trim()
      youtubeKey = ''
      toast(hasYoutubeKey ? t('libx.youtube_on') : t('libx.youtube_off'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('libx.key_failed'), true)
    }
  }
</script>

<h2 class="group">{t('libx.extras')}</h2>
<p class="small muted intro">
  {t('libx.intro')}
</p>

<section class="card">
  <label class="switch big">
    <input type="checkbox" bind:checked={settings.sound_analysis} onchange={() => save()} />
    <b>{t('libx.sound_analysis')}</b>
  </label>
  <p class="small muted">
    {t('libx.sound_analysis_note')}
  </p>
  {#if settings.sound_analysis && analysis}
    {@const total = analysis.analyzed + analysis.waiting}
    {#if analysis.held && analysis.waiting > 0}
      <p class="small">{heldText(analysis.held)}: {t('libx.done_of', { done: n(analysis.analyzed), total: n(total) })}</p>
    {:else if analysis.waiting > 0}
      <p class="small">
        {#if analysis.running}<span class="spinner" aria-hidden="true"></span>{/if}
        {t('libx.done_of', { done: n(analysis.analyzed), total: n(total) })}
      </p>
    {:else if total}
      <p class="small muted">{t('libx.all_analyzed', { n: n(total) })}</p>
    {/if}
  {/if}
</section>

<section class="card">
  <h3>{t('libx.heavy_when')}</h3>
  <p class="small muted">{t('libx.heavy_note')}</p>
  <div class="choices">
    <label class="switch">
      <input type="radio" value="any" bind:group={settings.background.when} onchange={() => save()} />
      {t('libx.any_time')}
    </label>
    <label class="switch">
      <input type="radio" value="idle" bind:group={settings.background.when} onchange={() => save()} />
      {t('libx.when_idle')}
    </label>
    <label class="switch">
      <input type="radio" value="hours" bind:group={settings.background.when} onchange={() => save()} />
      <span class="hours">
        {t('libx.only_from')}
        <select
          bind:value={settings.background.from}
          onchange={() => save()}
          disabled={settings.background.when !== 'hours'}
          aria-label={t('libx.from')}
        >
          {#each hours as h (h)}<option value={h}>{clock(h)}</option>{/each}
        </select>
        {t('libx.until')}
        <select
          bind:value={settings.background.until}
          onchange={() => save()}
          disabled={settings.background.when !== 'hours'}
          aria-label={t('libx.until_label')}
        >
          {#each hours as h (h)}<option value={h}>{clock(h)}</option>{/each}
        </select>
      </span>
    </label>
  </div>
  <p class="small muted">{t('libx.computer_time')}</p>
</section>

<section class="card">
  <label class="switch big">
    <input type="checkbox" bind:checked={settings.online_artist_info} onchange={() => save()} />
    <b>{t('libx.artist_info')}</b>
  </label>
  <p class="small muted">
    {t('libx.artist_info_note')}
  </p>
  <label class="switch sub">
    <input
      type="checkbox"
      bind:checked={settings.artist_pictures}
      disabled={!settings.online_artist_info}
      onchange={() => save()}
    />
    <span>
      {t('libx.more_pictures')}
      <span class="small muted">
        {t('libx.more_pictures_note')}
      </span>
    </span>
  </label>
  <div class="picture-limits">
    <h3>{t('libx.picture_limits')}</h3>
    <p class="small muted">{t('libx.picture_limits_note')}</p>
    <div class="limit-grid">
      <label>{t('libx.per_artist')}<input type="number" min="1" max="20" bind:value={settings.picture_settings.count} /></label>
      <label>{t('libx.mb_per_artist')}<input type="number" min="0" max="200" bind:value={settings.picture_settings.per_artist_mb} /></label>
      <label>{t('libx.cache_max')}<input type="number" min="0" max="100000" bind:value={settings.picture_settings.cache_mb} /></label>
      <label>{t('libx.max_edge')}<input type="number" min="512" max="4096" bind:value={settings.picture_settings.max_edge} /></label>
      <label>{t('libx.min_width')}<input type="number" min="0" max="4096" bind:value={settings.picture_settings.min_width} /></label>
      <label>{t('libx.min_height')}<input type="number" min="0" max="4096" bind:value={settings.picture_settings.min_height} /></label>
    </div>
    <button onclick={() => save(t('libx.limits_saved'))}>{t('libx.save_limits')}</button>
    {#if cache}
      <p class="small muted">{t('libx.cache_used', { used: mb(cache.used_bytes) })}{cache.max_bytes
          ? t('libx.cache_of', { max: mb(cache.max_bytes) })
          : t('libx.no_limit')}.</p>
      {#if cache.max_bytes}
        <progress max={cache.max_bytes} value={Math.min(cache.used_bytes, cache.max_bytes)} aria-label={t('libx.cache_bar')}></progress>
      {/if}
    {/if}
  </div>
  <div class="picture-limits">
    <h3>{t('libx.sources')}</h3>
    <p class="small muted">{t('libx.sources_note')}</p>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.deezer} /> {t('libx.src_deezer')}</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.wikipedia} /> {t('libx.src_wikipedia')}</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.commons} /> {t('libx.src_commons')}</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.fanart} /> {t('libx.src_fanart')}</label>
    <label class="field">{t('libx.fanart_key')}<SecretInput bind:value={settings.picture_settings.fanart_key} label={t('libx.fanart_key')} placeholder={t('libx.your_key')} /></label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.audiodb} /> {t('libx.src_audiodb')}</label>
    <label class="field">{t('libx.audiodb_key')}<SecretInput bind:value={settings.picture_settings.audiodb_key} label={t('libx.audiodb_key')} placeholder={t('libx.free_key')} /></label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.allow_remote} /> {t('libx.allow_remote')}</label>
    <label class="switch sub"><input type="checkbox" bind:checked={settings.picture_settings.unsplash} disabled={!settings.picture_settings.allow_remote} /> {t('libx.src_unsplash')}</label>
    <label class="field">{t('libx.unsplash_key')}<SecretInput bind:value={settings.picture_settings.unsplash_key} label={t('libx.unsplash_key')} placeholder={t('libx.unsplash_hint')} /></label>
    <button onclick={() => save(t('libx.sources_saved'))}>{t('libx.save_sources')}</button>
    <h3>{t('libx.browse')}</h3>
    <p class="small muted">{t('libx.browse_note')}</p>
    <div class="source-links">
      <a href="https://commons.wikimedia.org/wiki/Special:MediaSearch?type=image" target="_blank" rel="noopener noreferrer">Wikimedia Commons</a>
      <a href="https://fanart.tv/music-fanart/" target="_blank" rel="noopener noreferrer">fanart.tv</a>
      <a href="https://www.theaudiodb.com/" target="_blank" rel="noopener noreferrer">TheAudioDB</a>
      <a href="https://www.gettyimages.com/photos/famous-musicians" target="_blank" rel="noopener noreferrer">Getty Images</a>
      <a href="https://www.istockphoto.com/photos/music-artist" target="_blank" rel="noopener noreferrer">iStock</a>
      <a href="https://unsplash.com/s/photos/music-artist" target="_blank" rel="noopener noreferrer">Unsplash</a>
      <a href="https://photos.reelinintheyears.com/" target="_blank" rel="noopener noreferrer">Reelin' in the Years</a>
      <a href="https://photos.com/collections/music+lovers" target="_blank" rel="noopener noreferrer">Photos.com</a>
      <a href="https://www.rollingstone.com/product-recommendations/lifestyle/best-music-photography-prints-art-1052198/" target="_blank" rel="noopener noreferrer">{t('libx.rs_collections')}</a>
      <a href="https://www.pinterest.com/ideas/iconic-music-photos/929451450092/" target="_blank" rel="noopener noreferrer">{t('libx.pinterest')}</a>
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
    {t('libx.musicbrainz_note')}
  </p>
  <form
    class="inline"
    onsubmit={(e) => {
      e.preventDefault()
      save(t('common.saved'))
    }}
  >
    <label>
      {t('libx.contact')}
      <input
        type="text"
        bind:value={settings.musicbrainz_contact}
        placeholder="you@example.com"
        maxlength="200"
        autocapitalize="none"
      />
    </label>
    <button>{t('common.save')}</button>
  </form>
</section>

{#if lookups}
  <section class="card">
    <h3>{t('libx.looking_up')}</h3>
    {#if app.lookup.running}
      <p class="small"><span class="spinner" aria-hidden="true"></span> {t('libx.looking_now')}</p>
    {:else if app.lookup.last && app.lookup.finished_at}
      <p class="small muted">
        {t('libx.last_lookup', { when: when(app.lookup.finished_at), checked: app.lookup.last.checked })}{settings.online_artist_info
          ? t('libx.last_photos', { photos: app.lookup.last.photos, bios: app.lookup.last.bios })
          : ''}{settings.musicbrainz
          ? t('libx.last_covers', { covers: app.lookup.last.covers, genres: app.lookup.last.genres })
          : ''}{t('libx.found')}
      </p>
    {/if}
    <div class="row-buttons">
      <button disabled={app.lookup.running} onclick={() => lookUp(false)}>{t('libx.look_up_new')}</button>
      <button onclick={() => lookUp(true)}>{t('libx.recheck_all')}</button>
    </div>
  </section>
{/if}

<LyricsPanel />

<section class="card">
  <h3>{t('libx.identify')}</h3>
  <p class="small muted">
    {t('libx.identify_note')}
  </p>
  <form
    class="inline"
    onsubmit={(e) => {
      e.preventDefault()
      save(t('common.saved'))
    }}
  >
    <label>
      {t('libx.acoustid_key')}
      <SecretInput bind:value={settings.acoustid_key} label={t('libx.acoustid_key')} placeholder={t('libx.leave_empty')} maxlength={64} />
    </label>
    <button>{t('common.save')}</button>
  </form>
  <p class="small muted">
    {t('libx.acoustid_before')}
    <a href="https://acoustid.org/new-application" target="_blank" rel="noopener noreferrer">acoustid.org/new-application</a>
    {t('libx.acoustid_after')}
  </p>
</section>
<IdentifyPanel />

<section class="card">
  <h3>{t('libx.spotify', { state: spotifySaved?.client_id ? t('common.on') : t('common.off') })}</h3>
  <p class="small muted">
    {t('libx.spotify_note')}
  </p>
  <form
    class="inline"
    onsubmit={(e) => {
      e.preventDefault()
      void saveSpotify()
    }}
  >
    <label>
      {t('libx.client_id')}
      <input bind:value={spotifyId} autocomplete="off" spellcheck="false" maxlength="64" placeholder={t('libx.client_id_hint')} />
    </label>
    <label>
      {t('libx.client_secret')}
      <SecretInput
        bind:value={spotifySecret}
        label={t('libx.spotify_secret')}
        placeholder={spotifySaved?.has_secret ? t('libx.secret_saved') : t('libx.secret_hint')}
      />
    </label>
    <button disabled={spotifyBusy || !spotifyId.trim() || (!spotifySecret.trim() && !spotifySaved?.has_secret)}>
      {spotifyBusy ? t('common.checking') : t('common.save')}
    </button>
    {#if spotifySaved?.client_id}<button type="button" disabled={spotifyBusy} onclick={() => saveSpotify(true)}>{t('common.remove')}</button>{/if}
  </form>
  <p class="small muted">
    {t('libx.spotify_how_at')}
    <a href="https://developer.spotify.com/dashboard" target="_blank" rel="noopener noreferrer">developer.spotify.com/dashboard</a>,
    {t('libx.spotify_how_1')} <code>http://127.0.0.1:8066/</code> {t('libx.spotify_how_2')}
  </p>
</section>

<section class="card">
  <h3>{t('libx.youtube', { state: hasYoutubeKey ? t('common.on') : t('common.off') })}</h3>
  <p class="small muted">
    {t('libx.youtube_note')}
  </p>
  <form
    class="inline"
    onsubmit={(e) => {
      e.preventDefault()
      saveYoutubeKey(youtubeKey)
    }}
  >
    <label>
      {t('libx.youtube_key')}
      <SecretInput bind:value={youtubeKey} label={t('libx.youtube_key_label')} placeholder={hasYoutubeKey ? t('libx.key_saved') : t('libx.your_key')} />
    </label>
    <button disabled={!youtubeKey.trim()}>{t('common.save')}</button>
    {#if hasYoutubeKey}<button type="button" onclick={() => saveYoutubeKey('')}>{t('common.remove')}</button>{/if}
  </form>
  <p class="small muted">
    {t('libx.youtube_how_before')}
    <a href="https://console.cloud.google.com/apis/library/youtube.googleapis.com" target="_blank" rel="noopener noreferrer">Google Cloud</a>:
    {t('libx.youtube_how_after')}
  </p>
</section>

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
  .field :global(input) { width: 100%; }
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
