<script lang="ts">
  import { Archive, File, FilePdf, Image, FilmSlate } from 'phosphor-svelte';
  import type { TransferFileKind } from './types';
  export let kind: TransferFileKind = 'file';
  export let name = '';
  export let thumbnail: string | null = null;
</script>
<span class="file-visual" class:pdf={kind === 'pdf'}>
  {#if thumbnail}<img src={thumbnail} alt="" />
  {:else if kind === 'pdf'}<FilePdf size={36} weight="light" />
  {:else if kind === 'image'}<Image size={32} weight="light" />
  {:else if kind === 'archive'}<Archive size={32} weight="light" />
  {:else if /\.(mp4|mov|mkv|webm|avi)$/i.test(name)}<FilmSlate size={32} weight="light" />
  {:else}<File size={32} weight="light" />{/if}
</span>
<style>
  .file-visual { display: inline-flex; align-items: center; justify-content: center; width: 52px; height: 40px; flex: 0 0 52px; color: var(--text-secondary); }
  .file-visual.pdf { color: var(--danger); }
  img { width: 100%; height: 100%; object-fit: cover; border-radius: 5px; }
</style>
