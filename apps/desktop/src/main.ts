import '@fontsource-variable/archivo';
import '@fontsource-variable/inter';
import '@fontsource-variable/jetbrains-mono';
import '@fontsource-variable/source-sans-3';
import { mount } from 'svelte';

import App from './App.svelte';
import { initializeAppearance } from './lib/appearance';
import './styles.css';

initializeAppearance();

mount(App, {
  target: document.getElementById('app')!
});
