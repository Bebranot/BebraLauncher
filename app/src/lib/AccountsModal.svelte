<script lang="ts">
  import { api, errorText, type DenyCode } from "./api";
  import { formatDate } from "./format";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const denyText: Record<DenyCode, string> = {
    invalid_credentials: "Неверное имя или пароль.",
    account_unconfirmed: "Аккаунт не подтверждён: подтвердите почту.",
    tfa_required: "Нужен код двухфакторной аутентификации.",
    tfa_invalid: "Неверный код двухфакторной аутентификации.",
    account_locked: "Аккаунт заблокирован.",
    unknown: "Сервер отклонил вход.",
  };

  const stateText = { valid: "вход действует", renew: "вход действует", expired: "нужен вход" };

  // Logging in: a new name, or an existing account that has to log in again (userId).
  let username = $state("");
  let relogin = $state<{ userId: string; username: string } | null>(null);
  let password = $state("");
  let tfaCode = $state("");
  let askTfa = $state(false);
  let busy = $state(false);
  let message = $state<string | null>(null);

  function startRelogin(userId: string, name: string) {
    relogin = { userId, username: name };
    username = name;
    reset();
  }

  function reset() {
    password = "";
    tfaCode = "";
    askTfa = false;
    message = null;
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !password) return;
    busy = true;
    message = null;
    try {
      const out = await api.login({
        username: relogin ? undefined : username,
        userId: relogin?.userId,
        password,
        tfaCode: askTfa ? tfaCode : undefined,
      });
      if (out.status === "ok") {
        await app.loadAccounts();
        app.selectedAccount = out.account.user_id;
        username = "";
        relogin = null;
        reset();
      } else {
        if (out.code === "tfa_required" || out.code === "tfa_invalid") {
          askTfa = true;
          tfaCode = "";
        }
        message = [denyText[out.code], ...out.errors.filter((x) => x !== "")].join(" ");
        password = "";
      }
    } catch (err) {
      message = errorText(err);
      password = "";
    } finally {
      busy = false;
    }
  }

  async function logout(userId: string) {
    try {
      const told = await api.logout(userId);
      await app.loadAccounts();
      if (!told) {
        message =
          "Аккаунт удалён из лаунчера, но сервер входа не ответил: старый токен останется действительным до истечения.";
      }
    } catch (err) {
      message = errorText(err);
    }
  }
</script>

<Modal title="Аккаунты Wizards' Den" {onclose}>
  {#if app.accounts.length > 0}
    <ul class="accounts">
      {#each app.accounts as a (a.user_id)}
        <li>
          <div class="who">
            <b>{a.username}</b>
            <span class="dim small" class:error={a.state === "expired"}>
              {stateText[a.state]}{a.state !== "expired" ? ` до ${formatDate(a.expire_time)}` : ""}
            </span>
          </div>
          {#if a.state === "expired"}
            <button onclick={() => startRelogin(a.user_id, a.username)}>Войти заново</button>
          {/if}
          <button class="danger" onclick={() => logout(a.user_id)}>Выйти</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="dim">Нет сохранённых аккаунтов. Можно играть гостем на серверах без обязательной авторизации.</p>
  {/if}

  <form onsubmit={submit}>
    <h3>{relogin ? `Войти заново: ${relogin.username}` : "Добавить аккаунт"}</h3>
    {#if !relogin}
      <input type="text" placeholder="Имя аккаунта" bind:value={username} autocomplete="username" />
    {/if}
    <input
      type="password"
      placeholder="Пароль"
      bind:value={password}
      autocomplete="current-password"
    />
    {#if askTfa}
      <input
        type="text"
        placeholder="Код двухфакторной аутентификации"
        bind:value={tfaCode}
        inputmode="numeric"
        autocomplete="one-time-code"
      />
    {/if}
    {#if message}<div class="error small">{message}</div>{/if}
    <div class="actions">
      <button class="primary" type="submit" disabled={busy || !password || (!relogin && !username.trim())}>
        {busy ? "Вход…" : "Войти"}
      </button>
      {#if relogin}
        <button type="button" onclick={() => ((relogin = null), (username = ""), reset())}>Отмена</button>
      {/if}
    </div>
    <p class="dim small">
      Пароль уходит только на сервер входа Wizards' Den и нигде не сохраняется. Лаунчер хранит
      лишь токен — в Диспетчере учётных данных Windows.
    </p>
  </form>
</Modal>

<style>
  .accounts {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .who {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .small {
    font-size: 12px;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h3 {
    margin: 0;
    font-size: 14px;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  p {
    margin: 0;
  }
</style>
