<script lang="ts">
  import { onMount } from "svelte";
  import { translate as tr, language } from "../i18n";
  export let samples: number[] = [];
  export let boundary: number | null = null;
  let canvas: HTMLCanvasElement;
  let mounted = false;
  function draw(_samples = samples, _boundary = boundary) {
    if (!mounted || !canvas) return;
    const width = canvas.clientWidth;
    const height = 100;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = width * ratio;
    canvas.height = height * ratio;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(ratio, ratio);
    const styles = getComputedStyle(canvas);
    const y = (v: number) => 8 + ((-20 - v) / 90) * 84;
    if (_boundary !== null) {
      ctx.strokeStyle = styles.getPropertyValue("--warning");
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(0, y(_boundary));
      ctx.lineTo(width, y(_boundary));
      ctx.stroke();
    }
    ctx.setLineDash([]);
    ctx.strokeStyle = styles.getPropertyValue("--accent");
    ctx.lineWidth = 2;
    ctx.beginPath();
    _samples.forEach((v, i) => {
      const x = (i / Math.max(1, _samples.length - 1)) * width;
      if (i) ctx.lineTo(x, y(v));
      else ctx.moveTo(x, y(v));
    });
    ctx.stroke();
  }
  $: draw(samples, boundary);
  onMount(() => {
    mounted = true;
    const observer = new ResizeObserver(() => draw());
    observer.observe(canvas);
    draw();
    return () => observer.disconnect();
  });
</script>

<canvas bind:this={canvas} aria-label={tr("最近信号趋势；数值见上方实时读数", $language)}></canvas>

<style>
  canvas {
    display: block;
    width: 100%;
    height: 100px;
  }
</style>
