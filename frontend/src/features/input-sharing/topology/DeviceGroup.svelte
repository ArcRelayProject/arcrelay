<script lang="ts">
  export let name: string;
  export let x = 0;
  export let y = 0;
  export let width = 100;
</script>

<g class="group-label">
  <line x1={x} y1={y} x2={x + width} y2={y} />
  <text x={x} y={y - 8}>{name}</text>
</g>

<style>
  line { stroke: #cad1de; stroke-dasharray: 4 4; }
  text { fill: #667085; font-size: 11px; font-weight: 600; }
</style>
