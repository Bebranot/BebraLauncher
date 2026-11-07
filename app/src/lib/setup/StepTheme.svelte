<script lang="ts">
  import { t } from "../i18n.svelte";
  import Option from "../Option.svelte";
  import { app } from "../store.svelte";
  import { THEME_IDS, tokensFor } from "../themes";
  import Pick from "./Pick.svelte";

  const s = $derived(app.settings!);

  const options = $derived(
    THEME_IDS.map((id) => ({
      value: id as string,
      label: t(`theme.${id}`),
      tokens: tokensFor(id, s.custom_theme),
    })),
  );
</script>

<div class="su-stack">
  <Pick
    name="setup-theme"
    legend={t("opt.theme")}
    {options}
    value={s.theme}
    onpick={(v) => app.updateSettings({ theme: v })}
    min={128}
  >
    {#snippet lead(o)}
      {@const tk = options.find((x) => x.value === o.value)?.tokens}
      {#if tk}
        <span class="sample" style:background={tk.background} style:border-color={tk.separator}>
          <span class="panel" style:background={tk.popup}>
            <span class="line" style:background={tk.foreground}></span>
            <span class="line short" style:background={tk.sub}></span>
          </span>
          <span class="dot" style:background={tk.accent}></span>
        </span>
      {/if}
    {/snippet}
  </Pick>

  <div class="su-panel">
    <Option
      label={t("opt.gradient")}
      hint={t("setup.theme.gradientHint")}
      checked={s.theme_gradient}
      onchange={(v) => app.updateSettings({ theme_gradient: v })}
    />
    <Option
      label={t("opt.decor")}
      hint={t("setup.theme.decorHint")}
      checked={s.theme_decor}
      onchange={(v) => app.updateSettings({ theme_decor: v })}
    />
  </div>

  <p class="su-note">{t("setup.theme.custom")}</p>
</div>

<style>
  .sample {
    position: relative;
    display: block;
    height: 44px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .panel {
    position: absolute;
    left: 8px;
    top: 8px;
    bottom: 8px;
    width: 58%;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    padding: 0 8px;
    border-radius: 4px;
  }
  .line {
    display: block;
    height: 3px;
    border-radius: 2px;
    opacity: 0.85;
  }
  .line.short {
    width: 60%;
    opacity: 0.6;
  }
  .dot {
    position: absolute;
    right: 10px;
    top: 50%;
    width: 16px;
    height: 16px;
    margin-top: -8px;
    border-radius: 50%;
  }
</style>
