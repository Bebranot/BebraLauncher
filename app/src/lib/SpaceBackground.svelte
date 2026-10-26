<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { app } from "./store.svelte";

  let canvas = $state<HTMLCanvasElement>();
  let animId: number | null = null;

  interface Star {
    x: number;
    y: number;
    radius: number;
    baseAlpha: number;
    speed: number;
    phase: number;
    color: string;
    hasGlow: boolean;
  }

  let stars: Star[] = [];
  const STAR_COLORS = ["#ffffff", "#dbeafe", "#e0e7ff", "#fef3c7", "#cffafe"];

  function initStars(width: number, height: number) {
    const count = Math.min(180, Math.max(80, Math.floor((width * height) / 9000)));
    stars = [];
    for (let i = 0; i < count; i++) {
      const isBright = Math.random() < 0.12;
      stars.push({
        x: Math.random() * width,
        y: Math.random() * height,
        radius: isBright ? 1.6 + Math.random() * 1.2 : 0.6 + Math.random() * 0.9,
        baseAlpha: isBright ? 0.65 + Math.random() * 0.3 : 0.25 + Math.random() * 0.5,
        speed: 0.001 + Math.random() * 0.0025,
        phase: Math.random() * Math.PI * 2,
        color: STAR_COLORS[Math.floor(Math.random() * STAR_COLORS.length)],
        hasGlow: isBright,
      });
    }
  }

  function render(time: number) {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const w = canvas.width;
    const h = canvas.height;

    // Deep cosmic space background gradient
    const bgGrad = ctx.createLinearGradient(0, 0, w, h);
    bgGrad.addColorStop(0, "#06070b");
    bgGrad.addColorStop(0.5, "#090c14");
    bgGrad.addColorStop(1, "#07080f");
    ctx.fillStyle = bgGrad;
    ctx.fillRect(0, 0, w, h);

    // Subtle cosmic nebula glow (indigo and cyan dust clouds)
    const nebula1 = ctx.createRadialGradient(w * 0.25, h * 0.35, 10, w * 0.25, h * 0.35, Math.max(w, h) * 0.65);
    nebula1.addColorStop(0, "rgba(45, 25, 75, 0.18)");
    nebula1.addColorStop(0.5, "rgba(25, 35, 65, 0.08)");
    nebula1.addColorStop(1, "rgba(0, 0, 0, 0)");
    ctx.fillStyle = nebula1;
    ctx.fillRect(0, 0, w, h);

    const nebula2 = ctx.createRadialGradient(w * 0.8, h * 0.7, 10, w * 0.8, h * 0.7, Math.max(w, h) * 0.55);
    nebula2.addColorStop(0, "rgba(16, 52, 75, 0.15)");
    nebula2.addColorStop(0.6, "rgba(20, 30, 50, 0.05)");
    nebula2.addColorStop(1, "rgba(0, 0, 0, 0)");
    ctx.fillStyle = nebula2;
    ctx.fillRect(0, 0, w, h);

    // Render stars
    for (const star of stars) {
      const twinkle = Math.sin(time * star.speed + star.phase);
      const alpha = Math.max(0.1, Math.min(1.0, star.baseAlpha + twinkle * 0.3));

      ctx.save();
      ctx.globalAlpha = alpha;
      ctx.fillStyle = star.color;

      if (star.hasGlow) {
        ctx.shadowColor = star.color;
        ctx.shadowBlur = 6;
      }

      ctx.beginPath();
      ctx.arc(star.x, star.y, star.radius, 0, Math.PI * 2);
      ctx.fill();

      // Subtle diffraction spike on the brightest stars
      if (star.hasGlow && alpha > 0.7) {
        ctx.strokeStyle = star.color;
        ctx.lineWidth = 0.5;
        const spikeLen = star.radius * 2.5;
        ctx.beginPath();
        ctx.moveTo(star.x - spikeLen, star.y);
        ctx.lineTo(star.x + spikeLen, star.y);
        ctx.moveTo(star.x, star.y - spikeLen);
        ctx.lineTo(star.x, star.y + spikeLen);
        ctx.stroke();
      }

      ctx.restore();
    }
  }

  function startLoop() {
    if (animId !== null) return;
    if (document.hidden || (app.settings?.bg_type ?? "space") !== "space") return;
    animId = requestAnimationFrame(loop);
  }

  function stopLoop() {
    if (animId !== null) {
      cancelAnimationFrame(animId);
      animId = null;
    }
  }

  function loop(time: number) {
    animId = null;
    if (document.hidden || (app.settings?.bg_type ?? "space") !== "space") {
      return;
    }
    render(time);
    animId = requestAnimationFrame(loop);
  }

  function handleVisibilityChange() {
    if (document.hidden) {
      stopLoop();
    } else if ((app.settings?.bg_type ?? "space") === "space") {
      startLoop();
    }
  }

  function handleResize() {
    if (!canvas) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.floor(window.innerWidth * dpr);
    canvas.height = Math.floor(window.innerHeight * dpr);
    initStars(canvas.width, canvas.height);
    render(performance.now());
  }

  onMount(() => {
    handleResize();
    window.addEventListener("resize", handleResize);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    startLoop();
  });

  onDestroy(() => {
    stopLoop();
    window.removeEventListener("resize", handleResize);
    document.removeEventListener("visibilitychange", handleVisibilityChange);
  });

  const bgType = $derived(app.settings?.bg_type ?? "space");
  const bgDim = $derived(app.settings?.bg_dim ?? 40);
  const bgBlur = $derived(app.settings?.bg_blur ?? 0);
  const customImg = $derived(app.customBgUrl);

  $effect(() => {
    if (bgType === "space" && !document.hidden) {
      startLoop();
    } else {
      stopLoop();
    }
  });
</script>

<div class="bg-root" aria-hidden="true">
  {#if bgType === "space"}
    <canvas bind:this={canvas} class="space-canvas"></canvas>
  {:else if bgType === "custom" && customImg}
    <div
      class="custom-bg"
      style:background-image="url('{customImg}')"
      style:filter={bgBlur > 0 ? `blur(${bgBlur}px)` : "none"}
      style:transform={bgBlur > 0 ? "scale(1.06)" : "none"}
    ></div>
  {/if}

  {#if bgType === "custom" && bgDim > 0}
    <div class="dim-layer" style:background-color="rgba(0, 0, 0, {bgDim / 100})"></div>
  {:else if bgType === "space" && bgDim > 0}
    <div class="dim-layer" style:background-color="rgba(0, 0, 0, {(bgDim * 0.4) / 100})"></div>
  {/if}
</div>

<style>
  .bg-root {
    position: fixed;
    inset: 0;
    z-index: -1;
    pointer-events: none;
    overflow: hidden;
    background: #0a0b0e;
  }
  .space-canvas {
    display: block;
    width: 100%;
    height: 100%;
    position: absolute;
    inset: 0;
  }
  .custom-bg {
    position: absolute;
    inset: 0;
    background-size: cover;
    background-position: center;
    background-repeat: no-repeat;
    transition: filter 0.15s ease, transform 0.15s ease;
  }
  .dim-layer {
    position: absolute;
    inset: 0;
    transition: background-color 0.15s ease;
  }
</style>
