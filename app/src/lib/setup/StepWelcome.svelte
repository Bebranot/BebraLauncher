<script lang="ts">
  import { t } from "../i18n.svelte";
  import { nativeName, UI_LANGUAGES } from "../languages";
  import { app } from "../store.svelte";
  import Pick from "./Pick.svelte";
  import { STEP_IDS } from "./steps";

  const s = $derived(app.settings!);

  const options = $derived([
    { value: "auto", label: t("opt.langAuto") },
    ...UI_LANGUAGES.map((code) => ({ value: code as string, label: nativeName(code) })),
  ]);

  /** Someone who already has servers saved is updating, not installing. */
  const returning = $derived(s.favorites.length > 0 || s.history.length > 0);
  const topics = STEP_IDS.filter((id) => !["welcome", "import", "finish"].includes(id));
</script>

<div class="su-stack">
  <div class="su-block">
    <h3>{t("opt.language")}</h3>
    <Pick
      name="setup-language"
      legend={t("opt.language")}
      {options}
      value={s.language}
      onpick={(v) => app.updateSettings({ language: v })}
      min={130}
    />
  </div>

  <p class="su-note">{returning ? t("setup.welcome.returning") : t("setup.welcome.fresh")}</p>

  <div class="su-block">
    <h3>{t("setup.welcome.coverTitle")}</h3>
    <ul class="topics">
      {#each topics as id (id)}
        <li>{t(`setup.${id}.nav`)}</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .topics {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .topics li {
    padding: 3px 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
    font-size: 13px;
    color: var(--sub);
  }
</style>
