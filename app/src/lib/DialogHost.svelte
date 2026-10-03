<script lang="ts">
  import AddFavoriteDialog from "./AddFavoriteDialog.svelte";
  import DirectConnectDialog from "./DirectConnectDialog.svelte";
  import LoginDialog from "./LoginDialog.svelte";
  import { app } from "./store.svelte";

  const close = () => (app.dialog = null);
</script>

{#if app.dialog?.kind === "direct"}
  <DirectConnectDialog onclose={close} />
{:else if app.dialog?.kind === "addfav"}
  <AddFavoriteDialog onclose={close} />
{:else if app.dialog?.kind === "login"}
  <LoginDialog onclose={close} relogin={app.dialog.relogin} />
{/if}
