<script lang="ts">
  import { t } from "../i18n.svelte";
  import { app } from "../store.svelte";
  import type { SetupContext } from "./context.svelte";
  import Pick from "./Pick.svelte";
  import { gigabytes } from "./steps";

  let { ctx }: { ctx: SetupContext } = $props();

  const s = $derived(app.settings!);
  const hw = $derived(ctx.hardware);
  const suggested = $derived(hw?.suggested_profile ?? "balanced");

  const options = $derived([
    {
      value: "standard",
      label: t("setup.profile.standard"),
      hint: t("setup.profile.standardHint"),
    },
    {
      value: "balanced",
      label: t("setup.profile.balanced"),
      hint: t("setup.profile.balancedHint"),
    },
    {
      value: "optimized",
      label: t("setup.profile.optimized"),
      hint: t("setup.profile.optimizedHint"),
    },
    // Hand-tuned switches from Options stay as they are; the wizard only shows them.
    ...(s.opt_profile === "custom"
      ? [{ value: "custom", label: t("opt.profile.custom"), hint: t("setup.profile.customHint") }]
      : []),
  ].map((o) => ({ ...o, badge: o.value === suggested ? t("setup.profile.fits") : undefined })));

  // Measured on one computer (docs/performance-report.md); a hint of the order, not a promise.
  const FPS = [
    { id: "standard", fps: 127.4 },
    { id: "balanced", fps: 141.0 },
    { id: "optimized", fps: 193.1 },
  ] as const;
  const top = Math.max(...FPS.map((x) => x.fps));
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("setup.profile.yourPc")}</h3>
    {#if hw}
      <p class="su-note">
        {#if hw.ram_mb !== null}
          {t("setup.profile.pc", { cores: hw.cores, ram: gigabytes(hw.ram_mb) ?? 0 })}
        {:else}
          {t("setup.profile.pcNoRam", { cores: hw.cores })}
        {/if}
        {suggested === "optimized"
          ? t("setup.profile.adviceWeak")
          : t("setup.profile.adviceOk")}
      </p>
    {:else}
      <p class="su-note">{t("setup.profile.pcUnknown")}</p>
    {/if}
  </div>

  <Pick
    name="setup-profile"
    legend={t("opt.optProfile")}
    {options}
    value={s.opt_profile}
    onpick={(v) => app.updateSettings({ opt_profile: v })}
    min={210}
  />

  <div class="su-block">
    <h3>{t("setup.profile.benchTitle")}</h3>
    <ul class="bars">
      {#each FPS as row (row.id)}
        <li>
          <span class="who">{t(`setup.profile.${row.id}`)}</span>
          <span class="track" aria-hidden="true">
            <span class="fill" class:mine={row.id === s.opt_profile} style:width="{(row.fps / top) * 100}%"></span>
          </span>
          <span class="num">{Math.round(row.fps)}</span>
        </li>
      {/each}
    </ul>
    <p class="sub small">{t("setup.profile.benchNote")}</p>
  </div>
</div>

<style>
  .bars {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .bars li {
    display: grid;
    grid-template-columns: minmax(96px, 150px) 1fr 38px;
    align-items: center;
    gap: 10px;
    font-size: 14px;
  }
  .track {
    display: block;
    height: 10px;
    background: var(--tint);
    border-radius: 999px;
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    background: color-mix(in srgb, var(--accent) 45%, transparent);
    border-radius: 999px;
  }
  .fill.mine {
    background: var(--accent);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .small {
    margin: 0;
    font-size: 13px;
    line-height: 1.4;
  }
</style>
