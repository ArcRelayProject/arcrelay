<script lang="ts">
  import { appIcon128Url } from "./brandAssets";

  export let size = 24;
  export let alt = "";
</script>

<img
  class="brand-logo"
  src={appIcon128Url}
  {alt}
  width={size}
  height={size}
  decoding="async"
  draggable="false"
/>

<style>
  .brand-logo {
    display: block;
    flex: none;
    object-fit: contain;
  }
</style>
