<script lang="ts">
  import { t } from "../i18n.svelte";
  import { app } from "../store.svelte";
  import Pick from "./Pick.svelte";

  const s = $derived(app.settings!);

  const SYSTEM_FONT = "Segoe UI";
  const fontOptions = $derived([
    { value: "", label: t("opt.fontDefault") },
    { value: SYSTEM_FONT, label: t("opt.fontSystem") },
    // A font typed by hand in Options stays selectable, so the wizard cannot silently drop it.
    ...(s.font !== "" && s.font !== SYSTEM_FONT ? [{ value: s.font, label: s.font }] : []),
  ]);

  const bgOptions = $derived([
    { value: "space", label: t("opt.bgSpace"), hint: t("setup.look.bgSpaceHint") },
    { value: "none", label: t("opt.bgNone"), hint: t("setup.look.bgNoneHint") },
    ...(s.bg_type === "custom"
      ? [{ value: "custom", label: t("opt.bgCustom"), hint: t("setup.look.bgCustomHint") }]
      : []),
  ]);

  const scales = [75, 80, 90, 100, 110, 125, 150, 175, 200];
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("opt.font")}</h3>
    <Pick
      name="setup-font"
      legend={t("opt.font")}
      options={fontOptions}
      value={s.font}
      onpick={(v) => app.updateSettings({ font: v })}
      min={170}
    />
  </div>

  <div class="su-block">
    <h3>{t("opt.background")}</h3>
    <Pick
      name="setup-bg"
      legend={t("opt.bgMode")}
      options={bgOptions}
      value={s.bg_type}
      onpick={(v) => app.updateSettings({ bg_type: v })}
      min={200}
    />
    {#if s.bg_type !== "none"}
      <label class="su-field su-range">
        <span>{t("opt.bgDim", { value: s.bg_dim })}</span>
        <input
          type="range"
          min="0"
          max="90"
          step="5"
          value={s.bg_dim}
          oninput={(e) => app.updateSettings({ bg_dim: Number(e.currentTarget.value) })}
        />
      </label>
    {/if}
  </div>

  <div class="su-block">
    <h3>{t("opt.launcherUiScale")}</h3>
    <label class="su-field">
      <span class="sr-only">{t("opt.launcherUiScale")}</span>
      <select
        value={s.launcher_ui_scale ?? 100}
        onchange={(e) => app.updateSettings({ launcher_ui_scale: Number(e.currentTarget.value) })}
      >
        {#each scales as pct (pct)}
          <option value={pct}>{pct}%</option>
        {/each}
      </select>
      <small class="sub">{t("setup.look.scaleHint")}</small>
    </label>
  </div>
</div>
