<script lang="ts">
  import { t } from "../i18n.svelte";
  import Option from "../Option.svelte";
  import { app } from "../store.svelte";
  import Row from "./Row.svelte";

  const s = $derived(app.settings!);

  const fps = [60, 75, 120, 144, 165, 240];
  const scales = ["0.75", "1.0", "1.25", "1.5", "1.75", "2.0"];
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("setup.game.windowTitle")}</h3>
    <div class="su-panel">
      <Row label={t("opt.windowMode")} hint={t("setup.game.windowHint")}>
        <select
          value={s.client_window_mode}
          onchange={(e) => app.updateSettings({ client_window_mode: e.currentTarget.value })}
        >
          <option value="default">{t("opt.windowDefault")}</option>
          <option value="windowed">{t("opt.windowWindowed")}</option>
          <option value="fullscreen">{t("opt.windowFullscreen")}</option>
          <option value="borderless">{t("opt.windowBorderless")}</option>
        </select>
      </Row>
      <Row label={t("opt.uiScale")} hint={t("setup.game.scaleHint")}>
        <select
          value={s.client_ui_scale}
          onchange={(e) => app.updateSettings({ client_ui_scale: e.currentTarget.value })}
        >
          <option value="auto">{t("opt.uiScaleAuto")}</option>
          {#each scales as v (v)}
            <option value={v}>{Math.round(Number(v) * 100)}%</option>
          {/each}
        </select>
      </Row>
      <Row label={t("opt.maxFps")} hint={t("setup.game.fpsHint")}>
        <select
          value={s.client_max_fps}
          onchange={(e) => app.updateSettings({ client_max_fps: Number(e.currentTarget.value) })}
        >
          <option value={0}>{t("opt.maxFpsUncapped")}</option>
          {#each fps as v (v)}
            <option value={v}>{v} FPS</option>
          {/each}
        </select>
      </Row>
    </div>
  </div>

  <div class="su-block">
    <h3>{t("setup.game.otherTitle")}</h3>
    <div class="su-panel">
      <Option
        label={t("opt.muteUnfocused")}
        hint={t("opt.muteUnfocusedHint")}
        checked={s.client_mute_unfocused}
        onchange={(v) => app.updateSettings({ client_mute_unfocused: v })}
      />
      <Option
        label={t("opt.compat")}
        hint={t("opt.compatHint")}
        checked={s.compat_mode}
        onchange={(v) => app.updateSettings({ compat_mode: v })}
      />
      <label class="su-field">
        <span>{t("opt.guestName")}</span>
        <input
          type="text"
          maxlength="32"
          value={s.guest_name}
          oninput={(e) => app.updateSettings({ guest_name: e.currentTarget.value })}
        />
        <small class="sub">{t("opt.guestHint")}</small>
      </label>
    </div>
  </div>
</div>
