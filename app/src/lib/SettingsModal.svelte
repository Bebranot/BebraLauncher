<script lang="ts">
  import { api, errorText } from "./api";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";
  import { applyTheme, themes } from "./themes";

  let { onclose }: { onclose: () => void } = $props();

  let guestName = $state(app.settings?.guest_name ?? "");
  let compat = $state(app.settings?.compat_mode ?? false);
  let minimize = $state(app.settings?.minimize_on_play ?? true);
  let theme = $state(app.settings?.theme ?? "dark");

  // Preview while choosing; closing without saving puts the saved theme back.
  $effect(() => {
    applyTheme(theme);
    return () => applyTheme(app.settings?.theme ?? "dark");
  });
  let error = $state<string | null>(null);

  async function save() {
    if (!app.settings) return;
    try {
      await app.saveSettings({
        ...app.settings,
        guest_name: guestName,
        compat_mode: compat,
        minimize_on_play: minimize,
        theme,
      });
      onclose();
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<Modal title="Настройки" {onclose}>
  <div class="form">
    <label>
      <span>Имя гостя</span>
      <input type="text" bind:value={guestName} maxlength="32" />
      <small class="dim">Под этим именем игра запускается без аккаунта.</small>
    </label>

    <label>
      <span>Тема</span>
      <select bind:value={theme}>
        {#each themes as t}<option value={t.id}>{t.label}</option>{/each}
      </select>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={compat} />
      <span>
        Режим совместимости (GLES2)
        <small class="dim">Включите, если игра не запускается или не рисует графику.</small>
      </span>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={minimize} />
      <span>
        Сворачивать лаунчер, пока идёт игра
        <small class="dim">После закрытия игры окно вернётся.</small>
      </span>
    </label>

    <div class="about dim">
      <div>BebraLauncher {app.info?.version ?? ""}</div>
      <div class="path">Данные: {app.info?.data_dir ?? ""}</div>
      <button class="link" onclick={() => api.openLogsFolder()}>Открыть папку логов</button>
    </div>

    {#if error}<div class="error">{error}</div>{/if}
    <div class="actions">
      <button class="primary" onclick={save}>Сохранить</button>
      <button onclick={onclose}>Отмена</button>
    </div>
  </div>
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  label.check {
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
  }
  label.check span {
    display: flex;
    flex-direction: column;
  }
  small {
    font-size: 12px;
  }
  .about {
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .path {
    user-select: text;
    word-break: break-all;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    align-self: flex-start;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
</style>
