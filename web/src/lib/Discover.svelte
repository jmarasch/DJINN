<script lang="ts">
  // Shelves to find something to play: genres to dive into, what you played
  // lately, new albums, what's popular, artists like one you favorited, and a
  // few to explore. Everything links onward. Shared by the party app and
  // DJINN GO's web app, which give it their own song tiles and hearts.
  import type { Snippet } from 'svelte'
  import { api } from './api'
  import Art from './Art.svelte'
  import { t } from './i18n.svelte'
  import ScrollRow from './ScrollRow.svelte'
  import type { Album, ArtistSummary, Discover, Track } from './types'

  let {
    songTile,
    songRow,
    playAll,
    like,
  }: {
    /** A song as a row, for the quick picks. */
    songRow: Snippet<[Track]>
    /** Plays these songs one after another, when the app can (DJINN GO). */
    playAll?: (tracks: Track[]) => void
    /** A song as a tile; tapping it queues it (the party) or plays it (GO). */
    songTile: Snippet<[Track]>
    /** A heart for an artist or album, when the app has one. */
    like?: Snippet<['artist' | 'album', number, string]>
  } = $props()

  let found = $state<Discover | null>(null)
  let failed = $state(false)

  $effect(() => {
    api
      .discover()
      .then((d) => (found = d))
      .catch(() => (failed = true))
  })

  const nothing = $derived(
    found &&
      !found.speed_dial.length &&
      !found.mixes.length &&
      !found.genres.length &&
      !found.new_albums.length &&
      !found.popular_songs.length &&
      !found.explore.length,
  )
</script>

{#snippet artistTile(artist: ArtistSummary)}
  <div class="tile">
    <a href="#/artist/{artist.id}">
      <span class="frame round"><Art hash={artist.art_hash} size={130} round={999} letter={artist.name[0]} /></span>
      <span class="name ellipsis">{artist.name}</span>
      <span class="muted small ellipsis">{t('count.songs', { n: artist.track_count })}</span>
    </a>
    {#if like}<div class="corner">{@render like('artist', artist.id, artist.name)}</div>{/if}
  </div>
{/snippet}

{#snippet albumTile(album: Album)}
  <div class="tile">
    <a href="#/album/{album.id}">
      <span class="frame"><Art hash={album.art_hash} size={130} round={14} /></span>
      <span class="name ellipsis">{album.title}</span>
      <span class="muted small ellipsis">{album.artist}{album.year ? ` · ${album.year}` : ''}</span>
    </a>
    {#if like}<div class="corner">{@render like('album', album.id, album.title)}</div>{/if}
  </div>
{/snippet}

{#if failed}
  <p class="muted">Couldn't load this right now.</p>
{:else if !found}
  <p class="muted small">Loading...</p>
{:else if nothing}
  <p class="muted">Nothing to suggest yet. Add some music to the library.</p>
{:else}
  {#if found.speed_dial.length}
    <section>
      <h2>{t('discover.speed_dial')}</h2>
      <div class="dial">
        {#each found.speed_dial as item (item.kind + item.id)}
          <a class="dial-item" href="#/{item.kind}/{item.id}">
            <Art hash={item.art_hash} size={140} round={0} letter={item.name[0]} />
            <span class="dial-name ellipsis">{item.name}</span>
          </a>
        {/each}
      </div>
    </section>
  {/if}

  {#if found.genres.length}
    <section>
      <h2>{t('discover.genres')}</h2>
      <ScrollRow gap={8}>
        {#each found.genres as genre (genre.name)}
          <a class="pill" href="#/genre/{encodeURIComponent(genre.name)}">
            {genre.name} <span class="count">{genre.songs}</span>
          </a>
        {/each}
      </ScrollRow>
    </section>
  {/if}

  {#if found.listen_again.length}
    <section>
      <h2>{t('discover.listen_again')}</h2>
      <ScrollRow>
        {#each found.listen_again as track (track.id)}{@render songTile(track)}{/each}
      </ScrollRow>
    </section>
  {/if}

  {#if found.new_albums.length}
    <section>
      <h2>{t('discover.new_albums')}</h2>
      <ScrollRow>
        {#each found.new_albums as album (album.id)}
          <div class="tile">
            <a href="#/album/{album.id}">
              <span class="frame"><Art hash={album.art_hash} size={130} round={14} /></span>
              <span class="name ellipsis">{album.title}</span>
              <span class="muted small ellipsis">{album.artist}{album.year ? ` · ${album.year}` : ''}</span>
            </a>
            {#if like}<div class="corner">{@render like('album', album.id, album.title)}</div>{/if}
          </div>
        {/each}
      </ScrollRow>
    </section>
  {/if}

  {#if found.popular_songs.length}
    <section>
      <div class="head">
        <h2>{t('discover.quick_picks')}</h2>
        {#if playAll}
          <button class="small-button" onclick={() => playAll(found!.popular_songs)}>{t('discover.play_all')}</button>
        {/if}
      </div>
      <div class="picks">
        {#each found.popular_songs.slice(0, 6) as track (track.id)}{@render songRow(track)}{/each}
      </div>
    </section>
  {/if}

  {#if found.forgotten.length}
    <section>
      <h2>{t('discover.forgotten')}</h2>
      <ScrollRow>
        {#each found.forgotten as track (track.id)}{@render songTile(track)}{/each}
      </ScrollRow>
    </section>
  {/if}

  {#if found.mixes.length}
    <section>
      <h2>{t('discover.mixes')}</h2>
      <ScrollRow>
        {#each found.mixes as mix (mix.kind + mix.id + mix.name)}
          <div class="tile">
            <a href={mix.kind === 'artist' ? `#/artist/${mix.id}` : `#/genre/${encodeURIComponent(mix.name)}`}>
              <span class="frame"><Art hash={mix.art_hash} size={130} round={14} letter={mix.name[0]} /></span>
              <span class="name ellipsis">{t('discover.mix_named', { name: mix.name })}</span>
            </a>
          </div>
        {/each}
      </ScrollRow>
    </section>
  {/if}

  {#each found.more as shelf (shelf.kind)}
    <section>
      <h2>{t(`discover.${shelf.kind === 'genre' ? 'genre_more' : shelf.kind}`, { name: shelf.name })}</h2>
      <ScrollRow>
        {#each shelf.songs as track (track.id)}{@render songTile(track)}{/each}
        {#each shelf.albums as album (album.id)}{@render albumTile(album)}{/each}
        {#each shelf.artists as artist (artist.id)}{@render artistTile(artist)}{/each}
      </ScrollRow>
    </section>
  {/each}

  {#if found.throwbacks.length}
    <section>
      <h2>{t('discover.throwbacks')}</h2>
      <ScrollRow>
        {#each found.throwbacks as album (album.id)}
          <div class="tile">
            <a href="#/album/{album.id}">
              <span class="frame"><Art hash={album.art_hash} size={130} round={14} /></span>
              <span class="name ellipsis">{album.title}</span>
              <span class="muted small ellipsis">{album.artist}{album.year ? ` · ${album.year}` : ''}</span>
            </a>
            {#if like}<div class="corner">{@render like('album', album.id, album.title)}</div>{/if}
          </div>
        {/each}
      </ScrollRow>
    </section>
  {/if}

  {#if found.because}
    <section>
      <h2>
        {t('discover.because', { name: found.because.artist.name })}
      </h2>
      <ScrollRow>
        {#each found.because.similar as artist (artist.id)}{@render artistTile(artist)}{/each}
      </ScrollRow>
    </section>
  {/if}

  {#if found.explore.length}
    <section>
      <h2>{t('discover.explore')}</h2>
      <ScrollRow>
        {#each found.explore as artist (artist.id)}{@render artistTile(artist)}{/each}
      </ScrollRow>
    </section>
  {/if}
{/if}

<style>
  section {
    margin: 0 0 22px;
  }

  h2 {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px 12px;
    margin: 0 0 10px;
    color: var(--text);
    font-size: 1.125rem;
    text-shadow: 0 0 12px rgb(180 140 255 / 0.55);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .head {
    margin: 0 0 10px;
  }

  .head h2 {
    margin: 0;
  }

  .picks {
    display: grid;
    gap: 0 24px;
  }

  @media (min-width: 900px) {
    .picks {
      grid-template-columns: 1fr 1fr;
    }
  }

  .dial {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
  }

  @media (min-width: 900px) {
    .dial {
      grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    }
  }

  .dial-item {
    position: relative;
    display: block;
    aspect-ratio: 1;
    overflow: hidden;
    border: 1px solid var(--edge);
    border-radius: 14px;
    box-shadow: var(--glow);
    color: #fff;
    text-decoration: none;
  }

  .dial-item :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  .dial-name {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    padding: 18px 8px 6px;
    background: linear-gradient(transparent, rgb(0 0 0 / 0.75));
    font-size: 0.875rem;
    font-weight: 600;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 0 16px;
    border: 1px solid var(--edge);
    border-radius: 999px;
    background: var(--surface-2);
    box-shadow: var(--glow);
    color: var(--text);
    text-decoration: none;
    white-space: nowrap;
  }

  .pill:hover {
    border-color: var(--edge-strong);
    box-shadow: var(--glow-strong);
  }

  .count {
    color: var(--muted);
    font-size: 0.8125rem;
  }

  .tile {
    position: relative;
    flex: none;
    width: 140px;
  }

  .tile a {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--text);
    text-decoration: none;
  }

  .frame {
    display: block;
    width: 132px;
    height: 132px;
    overflow: hidden;
    border: 1px solid var(--edge);
    border-radius: 14px;
    box-shadow: var(--glow);
  }

  .frame.round {
    border-radius: 999px;
  }

  .frame :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  .name,
  .small {
    max-width: 132px;
  }

  .corner {
    position: absolute;
    top: 2px;
    right: 6px;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.45);
  }

  .corner :global(button:not(.liked)) {
    color: #fff;
  }
</style>
