<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../api";
  import { t } from "../i18n.svelte";
  import Option from "../Option.svelte";
  import { app } from "../store.svelte";

  const s = $derived(app.settings!);

  let autostart = $state(false);
  let links = $state(false);

  onMount(() => {
    void api.getAutostart().then((v) => (autostart = v)).catch(() => {});
    void api.getLinksRegistered().then((v) => (links = v)).catch(() => {});
  });

  async function setAutostart(on: boolean) {
    try {
      autostart = await api.setAutostart(on);
    } catch (e) {
      app.say(errorText(e));
      autostart = !on;
    }
  }

  async function setLinks(on: boolean) {
    try {
      links = await api.setLinksRegistered(on);
    } catch (e) {
      app.say(errorText(e));
      links = !on;
    }
  }
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("setup.system.windowsTitle")}</h3>
    <div class="su-panel">
      <Option label={t("opt.autostart")} hint={t("opt.autostartHint")} checked={autostart} onchange={setAutostart} />
      <Option label={t("opt.links")} hint={t("opt.linksHint")} checked={links} onchange={setLinks} />
    </div>
  </div>

  <div class="su-block">
    <h3>{t("setup.system.updatesTitle")}</h3>
    <div class="su-panel">
      <Option
        label={t("upd.optCheck")}
        hint={t("upd.optCheckHint")}
        checked={s.update_check}
        onchange={(v) => app.updateSettings({ update_check: v })}
      />
      <Option
        label={t("upd.optAuto")}
        hint={t("upd.optAutoHint")}
        checked={s.update_auto}
        onchange={(v) => app.updateSettings({ update_auto: v })}
      />
    </div>
  </div>

  <div class="su-block">
    <h3>{t("setup.system.diskTitle")}</h3>
    <div class="su-panel">
      <Option
        label={t("opt.autoCleanCache")}
        hint={t("opt.autoCleanCacheDesc")}
        checked={s.auto_clean_cache ?? true}
        onchange={(v) => app.updateSettings({ auto_clean_cache: v })}
      />
    </div>
  </div>
</div>
