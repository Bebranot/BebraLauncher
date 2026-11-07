<script lang="ts">
  import { t } from "../i18n.svelte";
  import Option from "../Option.svelte";
  import { app } from "../store.svelte";
  import Pick from "./Pick.svelte";
  import { flagsOf, playModeOf, type PlayMode } from "./steps";

  const s = $derived(app.settings!);
  const mode = $derived(playModeOf(s));

  const options = $derived<{ value: PlayMode; label: string; hint: string }[]>([
    { value: "stay", label: t("setup.launch.stay"), hint: t("setup.launch.stayHint") },
    { value: "minimize", label: t("setup.launch.minimize"), hint: t("opt.minimizeHint") },
    { value: "unload", label: t("setup.launch.unload"), hint: t("opt.unloadOnPlayHint") },
    { value: "close", label: t("setup.launch.close"), hint: t("opt.closeOnPlayHint") },
  ]);
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("setup.launch.whileTitle")}</h3>
    <Pick
      name="setup-play"
      legend={t("setup.launch.whileTitle")}
      {options}
      value={mode}
      onpick={(v) => app.updateSettings(flagsOf(v))}
      min={200}
    />
  </div>

  <div class="su-block">
    <h3>{t("setup.launch.safetyTitle")}</h3>
    <div class="su-panel">
      <Option
        label={t("opt.confirmConnect")}
        hint={t("opt.confirmConnectHint")}
        checked={s.confirm_connect}
        onchange={(v) => app.updateSettings({ confirm_connect: v })}
      />
      <Option
        label={t("opt.ignoreRedial")}
        hint={t("opt.ignoreRedialHint")}
        checked={s.ignore_redial}
        onchange={(v) => app.updateSettings({ ignore_redial: v })}
      />
    </div>
  </div>
</div>
