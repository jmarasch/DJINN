<script lang="ts">
  // Your own account, from the party app.
  import AccountPanels from '../lib/AccountPanels.svelte'
  import LanguagePicker from '../lib/LanguagePicker.svelte'
  import { app, logout } from '../lib/state.svelte'
</script>

<h1>Your account</h1>
<p class="muted">Signed in as <b>{app.me?.username}</b>{#if app.me?.role_name} ({app.me.role_name}){/if}.</p>
<section class="card language"><LanguagePicker /></section>
<AccountPanels />
<button class="signout" onclick={logout}>Sign out</button>

<style>
  .language :global(label) {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: flex-start;
  }

  .signout {
    width: 100%;
    margin: 8px 0 24px;
  }
</style>
