import { mount } from 'svelte';
import '../../src/styles.css';
import ScratchpadFixture from './ScratchpadFixture.svelte';

mount(ScratchpadFixture, { target: document.getElementById('app')! });
