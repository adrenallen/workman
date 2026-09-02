import '@fontsource-variable/archivo';
import '@fontsource-variable/inter';
import '@fontsource-variable/jetbrains-mono';
import '@fontsource-variable/source-sans-3';
import { mount } from 'svelte';
import { getCurrentWindow } from '@tauri-apps/api/window';

import App from './App.svelte';
import RecordedFeedbackOverlay from './lib/RecordedFeedbackOverlay.svelte';
import RecordedFeedbackToolbar from './lib/RecordedFeedbackToolbar.svelte';
import { initializeAppearance } from './lib/appearance';
import { installExternalLinkGuard } from './lib/externalLinks';
import './styles.css';

initializeAppearance();
installExternalLinkGuard();

const label = getCurrentWindow().label;
const Root = label === 'feedback-toolbar'
  ? RecordedFeedbackToolbar
  : label.startsWith('feedback-overlay-')
    ? RecordedFeedbackOverlay
    : App;

mount(Root, {
  target: document.getElementById('app')!
});
