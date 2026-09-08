import { mount } from 'svelte';
import '@fontsource-variable/inter';
import '../../src/styles.css';
import SettingsFixture from './SettingsFixture.svelte';
mount(SettingsFixture, { target: document.getElementById('app')! });
