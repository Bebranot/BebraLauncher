<script lang="ts">
  import { UNSPECIFIED, emptyFilters, languageName, regionName, rpKey } from "./filters";
  import { i18n, t, type Key } from "./i18n.svelte";
  import { app } from "./store.svelte";

  const RP: string[] = ["none", "low", "med", "high"];

  /** Ticks or unticks a value of a category. */
  function flip(list: string[], value: string, on: boolean): string[] {
    return on ? [...list.filter((v) => v !== value), value] : list.filter((v) => v !== value);
  }

  function count(text: string): number | null {
    const n = Number.parseInt(text, 10);
    return Number.isFinite(n) && n >= 0 ? n : null;
  }
</script>

<div class="panel">
  <!-- Resetting belongs to the whole panel, not to the players section (P2-54). -->
  <div class="panel-head">
    <h3>{t("filters.title")}</h3>
    <button class="link reset" onclick={() => (app.filters = emptyFilters())}>
      {t("filters.reset")}
    </button>
  </div>
  <div class="grid">
    <section>
      <h3>{t("filters.language")}</h3>
      {#each [...app.languages, UNSPECIFIED] as value (value)}
        <label>
          <input
            type="checkbox"
            checked={app.filters.languages.includes(value)}
            onchange={(e) =>
              (app.filters.languages = flip(app.filters.languages, value, e.currentTarget.checked))}
          />
          {value === UNSPECIFIED ? t("filters.unspecified") : languageName(value)}
        </label>
      {/each}
    </section>

    <section>
      <h3>{t("filters.region")}</h3>
      {#each [...app.regions, UNSPECIFIED] as value (value)}
        <label>
          <input
            type="checkbox"
            checked={app.filters.regions.includes(value)}
            onchange={(e) =>
              (app.filters.regions = flip(app.filters.regions, value, e.currentTarget.checked))}
          />
          {value === UNSPECIFIED ? t("filters.unspecified") : regionName(value, i18n.lang)}
        </label>
      {/each}
    </section>

    <section>
      <h3>{t("filters.rp")}</h3>
      {#each [...RP, UNSPECIFIED] as value (value)}
        <label>
          <input
            type="checkbox"
            checked={app.filters.rp.includes(value)}
            onchange={(e) => (app.filters.rp = flip(app.filters.rp, value, e.currentTarget.checked))}
          />
          {value === UNSPECIFIED ? t("filters.unspecified") : t(rpKey(value) ?? ("filters.rpNone" as Key))}
        </label>
      {/each}

      <h3 class="gap">{t("filters.age")}</h3>
      {#each [["yes", "filters.yes"], ["no", "filters.no"]] as [value, label] (value)}
        <label>
          <input
            type="checkbox"
            checked={app.filters.age.includes(value)}
            onchange={(e) => (app.filters.age = flip(app.filters.age, value, e.currentTarget.checked))}
          />
          {t(label as Key)}
        </label>
      {/each}
    </section>

    <section>
      <h3>{t("filters.players")}</h3>
      <label>
        <input type="checkbox" bind:checked={app.filters.hideEmpty} />
        {t("filters.hideEmpty")}
      </label>
      <label>
        <input type="checkbox" bind:checked={app.filters.hideFull} />
        {t("filters.hideFull")}
      </label>
      <label class="num">
        <span class="sub">{t("filters.min")}</span>
        <input
          type="number"
          min="0"
          aria-label={t("filters.min")}
          value={app.filters.minPlayers ?? ""}
          oninput={(e) => app.setPlayerRange("min", count(e.currentTarget.value))}
        />
      </label>
      <label class="num">
        <span class="sub">{t("filters.max")}</span>
        <input
          type="number"
          min="0"
          aria-label={t("filters.max")}
          value={app.filters.maxPlayers ?? ""}
          oninput={(e) => app.setPlayerRange("max", count(e.currentTarget.value))}
        />
      </label>
    </section>
  </div>
</div>

<style>
  .panel {
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: auto;
    height: 100%;
    padding: 12px 16px 14px;
    animation: fade-in 0.14s ease-out;
  }
  .panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 8px;
  }
  h3 {
    margin-bottom: 4px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--sub);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    gap: 10px 18px;
    align-items: start;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .gap {
    margin-top: 10px;
  }
  label {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    cursor: pointer;
  }
  .num {
    align-items: center;
  }
  .num input {
    width: 80px;
  }
  .reset {
    font-size: 12px;
  }
</style>
