<script lang="ts">
  // Admin > System > Language: what every app shows (party app, DJINN GO on
  // the web and on Android) unless someone picks their own on their account
  // page.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { house, languages, loadHouseLanguage, ownPick, setLanguage, t } from './i18n.svelte'
  import { toast } from './state.svelte'

  let value = $state(house.code)
  let loaded = $state(false)
  // This browser's own pick from Your Account, which wins over the house's.
  let own = $state(ownPick())
  const nameOf = (code: string) => languages.find((l) => l.code === code)?.name ?? code

  function followHouse() {
    setLanguage('')
    own = ''
  }

  onMount(async () => {
    try {
      value = (await api.language()).default
      loaded = true
    } catch {
      toast(t('hl.load_failed'), true)
    }
  })

  async function save() {
    try {
      await api.setLanguage(value)
      await loadHouseLanguage()
      toast(value ? t('hl.now_shows', { name: nameOf(value) }) : t('hl.each_own'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    }
  }
</script>

<section class="card">
  <h2>{t('hl.title')}</h2>
  <label class="field">
    {t('hl.default')}
    <select bind:value onchange={save} disabled={!loaded}>
      <option value="">{t('hl.device_own')}</option>
      {#each languages as language (language.code)}
        <option value={language.code}>{language.name}</option>
      {/each}
    </select>
  </label>
  <p class="small muted">
    {t('hl.note')}
  </p>
  {#if own}
    <p class="own small">
      {t('hl.own', { name: nameOf(own) })}
      <button class="link" onclick={followHouse}>{t('hl.follow')}</button>
    </p>
  {/if}
</section>

<style>
  .own {
    margin-top: 8px;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    min-height: 0;
    display: inline;
    line-height: inherit;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 320px;
  }
</style>
