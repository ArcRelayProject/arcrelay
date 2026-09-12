import { mount } from 'svelte';
import TrayTransfer from './TrayTransfer.svelte';
import '../fonts.css';
import '../styles/base.css';
import './tray-transfer.css';
import { installFrontendLogging } from '../logging';
import { installVisualPreviewReadiness, visualPreviewEnabled } from '../visualPreview';

installFrontendLogging();
const native = '__TAURI_INTERNALS__' in window && !visualPreviewEnabled();
document.documentElement.classList.toggle('tray-preview', !native);
const { port } = native ? await import('./native') : await import('./preview');
mount(TrayTransfer, { target: document.getElementById('app')!, props: { port } });
installVisualPreviewReadiness();
