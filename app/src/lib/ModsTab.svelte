<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type PatchRow } from "./api";
  import { formatBytes } from "./format";
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./store.svelte";

  const s = $derived(app.settings);

  let patches = $state<PatchRow[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let search = $state("");
  let fileInput = $state<HTMLInputElement>();
  let installing = $state(false);

  /** A `.dll` is read whole into a base64 string; a huge file would hang the window first. */
  const MAX_PATCH_BYTES = 32 * 1024 * 1024;

  const enabledCount = $derived(
    patches.filter((p) => s?.enabled_patches?.includes(p.id)).length,
  );

  const filteredPatches = $derived(
    patches.filter((p) => {
      const q = search.trim().toLowerCase();
      if (!q) return true;
      return (
        p.name.toLowerCase().includes(q) ||
        p.id.toLowerCase().includes(q)
      );
    }),
  );

  onMount(() => {
    void loadPatches();
  });

  async function loadPatches() {
    loading = true;
    loadError = null;
    try {
      patches = await api.listPatches();
    } catch (e) {
      loadError = errorText(e);
      app.say(errorText(e));
    } finally {
      loading = false;
    }
  }

  function togglePatch(patch: PatchRow, on: boolean) {
    if (!s) return;
    const rest = s.enabled_patches.filter((id) => id !== patch.id);
    app.updateSettings({ enabled_patches: on ? [...rest, patch.id] : rest });
  }

  async function deletePatch(patch: PatchRow) {
    const ok = window.confirm(t("mods.deleteConfirm", { name: patch.name }));
    if (!ok) return;

    try {
      await api.deletePatch(patch.id);
      await loadPatches();
    } catch (e) {
      app.say(errorText(e));
    }
  }

  function triggerFilePick() {
    fileInput?.click();
  }

  async function onFileSelected(e: Event) {
    const target = e.target as HTMLInputElement;
    const file = target.files?.[0];
    if (!file) return;

    // Check before `readAsDataURL`: a 500 MB file would be read into memory for nothing.
    if (file.size > MAX_PATCH_BYTES) {
      target.value = "";
      app.say(t("mods.fileTooLarge", { size: formatBytes(MAX_PATCH_BYTES) }));
      return;
    }

    installing = true;
    try {
      const reader = new FileReader();
      const base64Promise = new Promise<string>((resolve, reject) => {
        reader.onload = () => resolve(reader.result as string);
        reader.onerror = reject;
      });
      reader.readAsDataURL(file);
      const dataUrl = await base64Promise;
      await api.installPatch(file.name, dataUrl);
      target.value = "";
      await loadPatches();
    } catch (err) {
      app.say(errorText(err));
    } finally {
      installing = false;
    }
  }
</script>

<input
  bind:this={fileInput}
  type="file"
  accept=".dll"
  style="display: none;"
  onchange={onFileSelected}
/>

<div class="mods-tab" data-scroll>
  <!-- Header & Toolbar -->
  <header class="mods-header">
    <div class="header-main">
      <h2>{t("mods.title")}</h2>
      <p class="sub">{t("mods.desc")}</p>
    </div>

    <div class="toolbar">
      <div class="search-box">
        <Icon name="search" size={16} />
        <input
          type="text"
          placeholder={t("mods.search")}
          bind:value={search}
        />
        {#if search}
          <button
            class="icon-btn-clear"
            onclick={() => (search = "")}
            aria-label={t("servers.clearSearch")}
          >
            <Icon name="close" size={14} />
          </button>
        {/if}
      </div>

      <div class="actions">
        <button
          class="btn small primary"
          disabled={installing}
          onclick={triggerFilePick}
        >
          <Icon name="plus" size={16} />
          {t("mods.install")}
        </button>

        <button
          class="btn small"
          onclick={() => api.openPatchesFolder()}
          title={t("mods.openFolder")}
        >
          <Icon name="folder" size={16} />
          {t("mods.openFolder")}
        </button>

        <button
          class="btn small"
          disabled={loading}
          onclick={loadPatches}
          title={t("mods.refresh")}
        >
          <Icon name="refresh" size={16} />
          {t("mods.refresh")}
        </button>
      </div>
    </div>

    {#if patches.length > 0}
      <div class="stats-bar">
        <span class="badge count">
          {t("mods.enabledCount", { enabled: enabledCount, total: patches.length })}
        </span>
        {#if enabledCount > 0}
          <button
            class="btn small"
            onclick={() => app.updateSettings({ enabled_patches: [] })}
          >
            {t("mods.disableAll")}
          </button>
        {/if}
      </div>
      <!-- Executable code, not a translation pack: say so before it is used (P1-2). -->
      <div class="code-warning" role="note">
        <Icon name="note" size={16} />
        <span>{t("mods.codeWarning")}</span>
      </div>
      {#if enabledCount > 0}
        <div class="code-warning active" role="status">
          <span>{t("mods.activeWarning", { count: enabledCount })}</span>
        </div>
      {/if}
    {/if}
  </header>

  <!-- Mod list or Empty state -->
  <div class="mods-body">
    {#if loading && patches.length === 0}
      <div class="empty-state" role="status" aria-live="polite">
        <span class="spinner"></span>
        <p class="sub">{t("mods.loading")}</p>
      </div>
    {:else if loadError && patches.length === 0}
      <div class="empty-state">
        <p class="error">{t("mods.loadFailed", { error: loadError })}</p>
        <button class="btn small" onclick={loadPatches}>{t("mods.retry")}</button>
      </div>
    {:else if patches.length === 0}
      <div class="empty-state">
        <div class="empty-icon">
          <Icon name="note" size={40} />
        </div>
        <h3>{t("mods.title")}</h3>
        <p class="sub">{t("mods.empty")}</p>
        <div class="empty-actions">
          <button class="btn primary" onclick={triggerFilePick}>
            <Icon name="plus" size={16} />
            {t("mods.install")}
          </button>
          <button class="btn" onclick={() => api.openPatchesFolder()}>
            <Icon name="folder" size={16} />
            {t("mods.openFolder")}
          </button>
        </div>
      </div>
    {:else if filteredPatches.length === 0}
      <div class="empty-state">
        <p class="sub">{t("mods.noneFound")}</p>
        <button class="btn small" onclick={() => (search = "")}>
          {t("servers.clearSearch")}
        </button>
      </div>
    {:else}
      <div class="mods-grid">
        {#each filteredPatches as patch (patch.id)}
          {@const isEnabled = s?.enabled_patches?.includes(patch.id) ?? false}
          <div class="mod-card" class:enabled={isEnabled}>
            <div class="mod-switch">
              <label class="switch-wrap">
                <input
                  type="checkbox"
                  role="switch"
                  aria-label={patch.name}
                  checked={isEnabled}
                  onchange={(e) => togglePatch(patch, e.currentTarget.checked)}
                />
                <span class="slider"></span>
              </label>
            </div>

            <div class="mod-info">
              <div class="mod-name-row">
                <span class="mod-title">{patch.name}</span>
                <span class="mod-size">{formatBytes(patch.file_size)}</span>
              </div>
              <div class="mod-filename">{patch.id}</div>
              {#if !patch.verified}
                <div class="mod-changed">{t("mods.changed")}</div>
              {/if}
            </div>

            <div class="mod-status">
              <span class="status-pill" class:active={isEnabled}>
                {isEnabled ? t("mods.toggleOn") : t("mods.toggleOff")}
              </span>
            </div>

            <div class="mod-actions">
              <button
                class="icon-btn delete-btn"
                onclick={() => deletePatch(patch)}
                title={t("mods.delete")}
                aria-label={t("mods.delete")}
              >
                <Icon name="trash" size={16} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- Guide / instructions for local testing -->
    <section class="guide-card">
      <div class="guide-header">
        <Icon name="note" size={18} />
        <h4>{t("mods.helpTitle")}</h4>
      </div>
      <ol class="guide-steps">
        <li>{t("mods.helpStep1")}</li>
        <li>{t("mods.helpStep2")}</li>
        <li>{t("mods.helpStep3")}</li>
        <li>{t("mods.helpStep4")}</li>
        <li>{t("mods.helpStep5")}</li>
      </ol>
    </section>
  </div>
</div>

<style>
  .mods-tab {
    /* The parent is a plain block with height: 100%, not a flex container: `flex: 1` had no
       effect, the height equalled the content and nothing could scroll. */
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 24px 32px;
    gap: 20px;
    overflow-y: auto;
  }

  .code-warning {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--tint);
    color: var(--sub);
    font-size: 12px;
    line-height: 1.45;
  }

  .code-warning.active {
    color: var(--fg);
    border-color: var(--accent);
  }

  .mod-changed {
    margin-top: 4px;
    font-size: 11px;
    color: var(--bad);
  }

  .mods-header {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .header-main h2 {
    margin: 0 0 6px;
    font-size: 20px;
    font-weight: 600;
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    min-width: 240px;
    color: var(--sub);
  }

  .search-box input {
    border: none;
    background: none;
    color: var(--fg);
    font-size: 13px;
    outline: none;
    flex: 1;
  }

  .icon-btn-clear {
    background: none;
    border: none;
    color: var(--sub);
    cursor: pointer;
    padding: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .icon-btn-clear:hover {
    color: var(--fg);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .stats-bar {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .badge.count {
    font-size: 12px;
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--tint);
    color: var(--sub);
    border: 1px solid var(--line);
  }

  .mods-body {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 48px 24px;
    background: var(--tint);
    border: 1px dashed var(--line);
    border-radius: var(--radius);
    gap: 12px;
  }

  .empty-icon {
    color: var(--sub);
    opacity: 0.6;
    margin-bottom: 4px;
  }

  .empty-state h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }

  .empty-actions {
    display: flex;
    gap: 10px;
    margin-top: 8px;
  }

  .mods-grid {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .mod-card {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 16px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    transition:
      border-color 0.15s ease,
      background 0.15s ease;
  }

  .mod-card:hover {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--line));
  }

  .mod-card.enabled {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--line));
    background: color-mix(in srgb, var(--accent) 4%, var(--tint));
  }

  /* Switch styling */
  .switch-wrap {
    position: relative;
    display: inline-block;
    width: 38px;
    height: 22px;
    flex-shrink: 0;
  }

  .switch-wrap input {
    opacity: 0;
    width: 0;
    height: 0;
  }

  .slider {
    position: absolute;
    cursor: pointer;
    inset: 0;
    background-color: color-mix(in srgb, var(--fg) 20%, transparent);
    transition: 0.2s;
    border-radius: 999px;
  }

  .slider:before {
    position: absolute;
    content: "";
    height: 16px;
    width: 16px;
    left: 3px;
    bottom: 3px;
    background-color: #fff;
    transition: 0.2s;
    border-radius: 50%;
  }

  .switch-wrap input:checked + .slider {
    background-color: var(--accent);
  }

  .switch-wrap input:checked + .slider:before {
    transform: translateX(16px);
  }

  /* The box itself is invisible: the ring has to go on the track (P2-51). */
  .switch-wrap input:focus-visible + .slider {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .mod-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .mod-name-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .mod-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--fg);
  }

  .mod-size {
    font-size: 12px;
    color: var(--sub);
  }

  .mod-filename {
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
    color: var(--sub);
    opacity: 0.8;
  }

  .status-pill {
    font-size: 11px;
    font-weight: 500;
    padding: 3px 8px;
    border-radius: var(--radius-sm);
    background: var(--tint);
    color: var(--sub);
    white-space: nowrap;
  }

  .status-pill.active {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
    font-weight: 600;
  }

  .icon-btn {
    appearance: none;
    border: none;
    background: none;
    color: var(--sub);
    cursor: pointer;
    padding: 6px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    transition:
      background 0.12s,
      color 0.12s;
  }

  .icon-btn:hover {
    background: var(--tint-hover);
    color: var(--fg);
  }

  .delete-btn:hover {
    background: color-mix(in srgb, #e53935 15%, transparent);
    color: #e53935;
  }

  /* Guide card */
  .guide-card {
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .guide-header {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--fg);
  }

  .guide-header h4 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }

  .guide-steps {
    margin: 0;
    padding: 0 0 0 18px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    line-height: 1.5;
    color: var(--sub);
  }

  .guide-steps li {
    margin: 0;
  }
</style>
