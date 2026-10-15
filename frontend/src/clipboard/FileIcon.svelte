<script lang="ts">
  import { AndroidLogo, FileZip, FilePdf, FileImage, FileText } from "phosphor-svelte";

  export let name: string;
  export let compact = false;
  $: extension = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  $: archive = ["zip", "rar", "7z", "gz", "tar", "bz2", "xz"].includes(extension);
  $: image = ["png", "jpg", "jpeg", "gif", "webp", "svg", "heic"].includes(extension);
</script>

<span class="file-icon" class:compact class:apk={extension === "apk"} aria-hidden="true">
  {#if extension === "apk"}<AndroidLogo size={compact ? 16 : 21} />
  {:else if archive}<FileZip size={compact ? 16 : 21} />
  {:else if extension === "pdf"}<FilePdf size={compact ? 16 : 21} />
  {:else if image}<FileImage size={compact ? 16 : 21} />
  {:else}<FileText size={compact ? 16 : 21} />{/if}
</span>

<style>
  .file-icon {
    display: inline-flex;
    width: 28px;
    height: 32px;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    color: var(--accent-strong);
    background: var(--accent-soft);
  }
  .file-icon.apk {
    color: #2e8549;
    background: #eaf7ee;
  }
  :global(:root[data-theme="dark"]) .file-icon.apk {
    color: #78d895;
    background: #223d2d;
  }
  .file-icon.compact {
    width: 22px;
    height: 24px;
    border-radius: 4px;
  }
</style>
