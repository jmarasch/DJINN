<script lang="ts">
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { About } from './types'

  // Which card: the build, or the credits. Both when not said.
  let { part = 'both' }: { part?: 'both' | 'build' | 'credits' } = $props()

  let about = $state<About | null>(null)

  $effect(() => {
    api
      .about()
      .then((a) => (about = a))
      .catch((e) => toast(e.message, true))
  })
</script>

{#if about}
  {#if part !== 'credits'}
  <section class="card">
    <img class="shiny" src="/images/logo.webp" alt="" />
    <h2>DJINN {about.version}</h2>
    <p class="small muted">
      A party jukebox by New Terra Studios.{#if about.commit}{' '}Build {about.commit}{about.commit_date ? `, ${about.commit_date}` : ''}.{/if}
    </p>
    <p class="small">
      Free to use at home, for hobbies, at school, for research and for charities. If you want to use it to make
      money, such as in a bar or venue, ask first.
    </p>
    <p class="small muted">
      The full terms: free for any non-commercial use under the
      <a href="https://polyformproject.org/licenses/noncommercial/1.0.0" target="_blank" rel="noopener noreferrer"
        >PolyForm Noncommercial License 1.0.0</a
      >. Nobody may sell it or use it to make money.
    </p>
    <div class="links">
      <a href={about.repository} target="_blank" rel="noopener noreferrer">Source code on GitHub</a>
      <a href="{about.repository}/issues" target="_blank" rel="noopener noreferrer">Report a problem</a>
      <a href="{about.repository}/releases" target="_blank" rel="noopener noreferrer">All releases</a>
      <a href="mailto:{about.contact}">{about.contact}</a>
    </div>
  </section>
  {/if}

  {#if part !== 'build'}
  <section class="card">
    <h2>Credits</h2>
    <ul class="small credits">
      <li>Music details from MusicBrainz and the Cover Art Archive; song identification by AcoustID.</li>
      <li>Artist photos from Deezer and Wikimedia Commons; bios from Wikipedia.</li>
      <li>AI listening: LAION's CLAP (Apache-2.0). AI reader: Qwen2.5 by Alibaba Cloud (Apache-2.0).</li>
      <li>Built with Rust, Svelte, and many open source libraries, including Hugging Face's candle (MIT or Apache-2.0).</li>
    </ul>
  </section>
  {/if}
{/if}

<style>
  .shiny {
    align-self: center;
    width: 120px;
    height: 120px;
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
  }

  .credits {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
</style>
