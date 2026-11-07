<script lang="ts">
  import { t } from "../i18n.svelte";
  import { nativeName } from "../languages";
  import { app } from "../store.svelte";
  import { isThemeId } from "../themes";
  import { playModeOf } from "./steps";

  const s = $derived(app.settings!);

  const language = $derived(s.language === "auto" ? t("opt.langAuto") : nativeName(s.language));
  const theme = $derived(isThemeId(s.theme) ? t(`theme.${s.theme}`) : s.theme);
  const profile = $derived(
    s.opt_profile === "standard"
      ? t("setup.profile.standard")
      : s.opt_profile === "balanced"
        ? t("setup.profile.balanced")
        : s.opt_profile === "optimized"
          ? t("setup.profile.optimized")
          : t("opt.profile.custom"),
  );
  const playing = $derived(t(`setup.launch.${playModeOf(s)}`));
  const hubCount = $derived(s.hubs.filter((h) => h.enabled).length);
  const updates = $derived(
    !s.update_check
      ? t("setup.finish.updOff")
      : s.update_auto
        ? t("setup.finish.updAuto")
        : t("setup.finish.updCheck"),
  );
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("setup.finish.summary")}</h3>
    <dl class="su-where">
      <dt>{t("opt.language")}</dt>
      <dd>{language}</dd>
      <dt>{t("opt.theme")}</dt>
      <dd>{theme}</dd>
      <dt>{t("opt.optProfile")}</dt>
      <dd>{profile}</dd>
      <dt>{t("setup.launch.whileTitle")}</dt>
      <dd>{playing}</dd>
      <dt>{t("opt.hubs")}</dt>
      <dd>{t("setup.finish.hubs", { count: hubCount })}</dd>
      <dt>{t("setup.system.updatesTitle")}</dt>
      <dd>{updates}</dd>
    </dl>
  </div>

  <p class="su-note">{app.setupRerun ? t("setup.finish.againNote") : t("setup.finish.later")}</p>
</div>
