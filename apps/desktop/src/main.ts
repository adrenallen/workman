import '@fontsource-variable/archivo';
import '@fontsource-variable/jetbrains-mono';
import { mount } from 'svelte';

import App from './App.svelte';
import './styles.css';

mount(App, {
  target: document.getElementById('app')!
});
