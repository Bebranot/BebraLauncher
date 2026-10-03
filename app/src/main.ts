import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// Outside Tauri (plain browser during development) there is no backend: use fake data.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installMock } = await import("./dev-mock");
  installMock();
}

mount(App, { target: document.getElementById("app")! });
