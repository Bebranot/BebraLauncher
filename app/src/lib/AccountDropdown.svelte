<script lang="ts">
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./store.svelte";

  let open = $state(false);
  let root = $state<HTMLElement>();

  const current = $derived(app.selectedAccountRow);
  const label = $derived(
    current
      ? current.username
      : app.accounts.length > 0
        ? t("acct.guest", { name: app.settings?.guest_name ?? "" })
        : t("acct.notLoggedIn"),
  );

  function choose(userId: string | null) {
    app.selectedAccount = userId;
    open = false;
  }

  function addAccount() {
    open = false;
    app.dialog = { kind: "login" };
  }

  function relogin(userId: string, username: string) {
    open = false;
    app.dialog = { kind: "login", relogin: { userId, username } };
  }

  async function logout() {
    if (!current) return;
    open = false;
    const outcome = await app.logout(current.user_id);
    if (outcome === "offline") app.say(t("login.offline"), 14000);
  }

  // Closes on a click anywhere else, and on Escape.
  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === "Escape" && (open = false)} />

<div class="drop" bind:this={root}>
  <button class="btn open-both who" onclick={() => (open = !open)} aria-expanded={open}>
    <span class="name">{label}</span>
    <Icon name="chevron" size={18} />
  </button>

  {#if open}
    <div class="popup">
      {#if app.accounts.length > 0}
        <div class="sub small">{t("acct.select")}</div>
        {#each app.accounts as a (a.user_id)}
          <div class="row">
            <button
              class="pick"
              class:on={a.user_id === app.selectedAccount}
              onclick={() => (a.state === "expired" ? relogin(a.user_id, a.username) : choose(a.user_id))}
            >
              {a.username}
              {#if a.state === "expired"}<span class="error small">{t("acct.expiredShort")}</span>{/if}
            </button>
          </div>
        {/each}
        <div class="row">
          <button class="pick" class:on={app.selectedAccount === null} onclick={() => choose(null)}>
            {t("acct.guest", { name: app.settings?.guest_name ?? "" })}
          </button>
        </div>
        <div class="rule"></div>
      {/if}
      {#if current}
        <button class="pick" onclick={logout}>{t("acct.logoutOf", { name: current.username })}</button>
      {/if}
      <button class="pick" onclick={addAccount}>{t("acct.add")}</button>
    </div>
  {/if}
</div>

<style>
  .drop {
    position: relative;
  }
  .who {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 200px;
    padding: 5px 10px;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 240px;
  }
  .popup {
    position: absolute;
    right: 0;
    top: calc(100% + 2px);
    z-index: 20;
    min-width: 100%;
    width: max-content;
    max-width: 320px;
    background: var(--popup);
    border: 1px solid var(--ctrl-mid);
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    box-shadow: 0 4px 14px #000a;
  }
  .small {
    font-size: 12px;
    padding: 2px 6px;
  }
  .pick {
    appearance: none;
    background: none;
    border: 0;
    text-align: left;
    padding: 5px 8px;
    cursor: pointer;
    display: flex;
    gap: 10px;
    justify-content: space-between;
    width: 100%;
  }
  .pick:hover {
    background: var(--btn-hover);
  }
  .pick.on {
    background: var(--tab-selected);
  }
  .rule {
    margin: 3px 0;
  }
</style>
