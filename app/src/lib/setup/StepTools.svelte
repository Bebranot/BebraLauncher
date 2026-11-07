<script lang="ts">
  import { t } from "../i18n.svelte";

  const tabs = ["servers", "favorites", "recent", "news", "updates", "mods", "options"] as const;
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("setup.tools.tabsTitle")}</h3>
    <dl class="su-where">
      {#each tabs as id (id)}
        <dt>{t(`tab.${id}`)}</dt>
        <dd>{t(`setup.tools.tab.${id}`)}</dd>
      {/each}
    </dl>
  </div>

  <div class="su-block">
    <h3>{t("setup.tools.inOptionsTitle")}</h3>
    <dl class="su-where">
      <dt>{t("opt.toc.optimization")}</dt>
      <dd>{t("setup.tools.opt.optimization")}</dd>
      <dt>{t("opt.toc.mods")}</dt>
      <dd>{t("setup.tools.opt.mods")}</dd>
      <dt>{t("opt.toc.storage")}</dt>
      <dd>{t("setup.tools.opt.storage")}</dd>
      <dt>{t("opt.toc.config")}</dt>
      <dd>{t("setup.tools.opt.config")}</dd>
      <dt>{t("opt.toc.about")}</dt>
      <dd>{t("setup.tools.opt.about")}</dd>
    </dl>
  </div>
</div>
