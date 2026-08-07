import '@fontsource-variable/archivo';
import '@fontsource-variable/inter';
import '@fontsource-variable/jetbrains-mono';
import '@fontsource-variable/source-sans-3';
import { mount } from 'svelte';

import App from './App.svelte';
import { initializeAppearance } from './lib/appearance';
import { installExternalLinkGuard } from './lib/externalLinks';
import './styles.css';

initializeAppearance();
installExternalLinkGuard();

mount(App, {
  target: document.getElementById('app')!
});
