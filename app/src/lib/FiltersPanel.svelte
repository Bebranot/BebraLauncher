<script lang="ts">
  import { UNSPECIFIED, emptyFilters, languageName, regionName } from "./filters";
  import { i18n, t, type Key } from "./i18n.svelte";
  import { app } from "./store.svelte";

  const RP: [string, Key][] = [
    ["none", "filters.rpNone"],
    ["low", "filters.rpLow"],
    ["med", "filters.rpMed"],
    ["high", "filters.rpHigh"],
  ];

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
      {#each [...RP, [UNSPECIFIED, "filters.unspecified"] as [string, Key]] as [value, label] (value)}
        <label>
          <input
            type="checkbox"
            checked={app.filters.rp.includes(value)}
            onchange={(e) => (app.filters.rp = flip(app.filters.rp, value, e.currentTarget.checked))}
          />
          {t(label)}
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
      <div class="num">
        <span class="sub">{t("filters.min")}</span>
        <input
          type="number"
          min="0"
          value={app.filters.minPlayers ?? ""}
          oninput={(e) => (app.filters.minPlayers = count(e.currentTarget.value))}
        />
      </div>
      <div class="num">
        <span class="sub">{t("filters.max")}</span>
        <input
          type="number"
          min="0"
          value={app.filters.maxPlayers ?? ""}
          oninput={(e) => (app.filters.maxPlayers = count(e.currentTarget.value))}
        />
      </div>
      <button class="link reset" onclick={() => (app.filters = emptyFilters())}>
        {t("filters.reset")}
      </button>
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
  h3 {
    margin-bottom: 4px;
    font-size: 11px;
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
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .num input {
    width: 80px;
  }
  .reset {
    margin-top: 8px;
  }
</style>
