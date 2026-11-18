<script lang="ts">
  // Your own account, from the party app.
  import AccountPanels from '../lib/AccountPanels.svelte'
  import { app, logout } from '../lib/state.svelte'
  import { t } from '../lib/i18n.svelte'
</script>

<h1>{t('nav.your_account')}</h1>
<p class="muted">{t('acc.signed_in_as')} <b>{app.me?.username}</b>{#if app.me?.role_name} ({app.me.role_name}){/if}.</p>
<AccountPanels />
<button class="signout" onclick={logout}>{t('up.sign_out')}</button>

<style>
  .signout {
    width: 100%;
    margin: 8px 0 24px;
  }
</style>
