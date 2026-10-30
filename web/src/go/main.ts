// DJINN GO: personal listening, served on its own port.
import '../polyfills'
import { mount } from 'svelte'
import '../app.css'
import GoApp from './GoApp.svelte'
import { applyTheme } from '../lib/theme.svelte'

applyTheme()

const app = mount(GoApp, {
  target: document.getElementById('app')!,
})

export default app
