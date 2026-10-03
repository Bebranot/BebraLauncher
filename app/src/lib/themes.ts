// Colour themes. The colours themselves are CSS variables in app.css (`:root[data-theme=…]`).
export const themes = [
  { id: "dark", label: "theme.dark" },
  { id: "light", label: "theme.light" },
  { id: "ocean", label: "theme.ocean" },
] as const;

export function applyTheme(id: string | undefined): void {
  const known = themes.some((t) => t.id === id);
  document.documentElement.dataset.theme = known ? id : "dark";
}
