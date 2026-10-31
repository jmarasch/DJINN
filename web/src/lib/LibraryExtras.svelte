<script lang="ts">
  // Admin > Library: what a library check does beyond reading folders and
  // tags. Each extra is off until switched on; switches save right away,
  // text fields with their own Save.
  import { api } from './api'
  import { clock, heldText } from './background'
  import IdentifyPanel from './IdentifyPanel.svelte'
  import { app, toast } from './state.svelte'
  import type { Settings } from './types'

  let { settings = $bindable() }: { settings: Settings } = $props()

  async function save(done?: string) {
    try {
      settings = await api.saveSettings(settings)
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
      disabled={!settings.online_artist_info || (!settings.artist_pictures && !settings.musicbrainz)}
      onchange={() => save()}
    />
    <span>
      More pictures, as a slideshow on artist pages and on the TV page while their songs play
      <span class="small muted">
        (from Wikimedia Commons, with credits; needs MusicBrainz below to find the right artist. Each artist's
        pictures are fetched the first time they're shown.)
      </span>
    </span>
  </label>
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
