import { mount } from 'svelte';
import TrayTransfer from './TrayTransfer.svelte';
import '../fonts.css';
import '../styles/base.css';
import './tray-transfer.css';
import { installFrontendLogging } from '../logging';

installFrontendLogging();
const native = '__TAURI_INTERNALS__' in window;
document.documentElement.classList.toggle('tray-preview', !native);
const { port } = native ? await import('./native') : await import('./preview');
mount(TrayTransfer, { target: document.getElementById('app')!, props: { port } });
