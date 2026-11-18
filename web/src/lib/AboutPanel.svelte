<script lang="ts">
  import { t } from './i18n.svelte'
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
    {#if about.build_label}<p><b>{about.build_label}</b></p>{/if}
    <p class="small muted">
      {t('ab.by')}{#if about.commit}{' '}{t('ab.build', { commit: about.commit })}{about.commit_date ? `, ${about.commit_date}` : ''}.{/if}
    </p>
    <p class="small">
      {t('ab.free')}
    </p>
    <p class="small muted">
      {t('ab.terms_1')}
      <a href="https://polyformproject.org/licenses/noncommercial/1.0.0" target="_blank" rel="noopener noreferrer"
        >PolyForm Noncommercial License 1.0.0</a
      >{t('ab.terms_2')}
    </p>
    <div class="links">
      <a href={about.repository} target="_blank" rel="noopener noreferrer">{t('ab.source')}</a>
      <a href="{about.repository}/issues" target="_blank" rel="noopener noreferrer">{t('ab.report')}</a>
      <a href="{about.repository}/releases" target="_blank" rel="noopener noreferrer">{t('ab.releases')}</a>
      <a href="mailto:{about.contact}">{about.contact}</a>
    </div>
  </section>
  {/if}

  {#if part !== 'build'}
  <section class="card">
    <h2>{t('ab.credits')}</h2>
    <ul class="small credits">
      <li>{t('ab.c1')}</li>
      <li>{t('ab.c2')}</li>
      <li>{t('ab.c3')}</li>
      <li>{t('ab.c4')}</li>
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
