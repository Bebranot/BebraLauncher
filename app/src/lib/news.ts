// The launcher's own news: choosing a text for the interface language and what is still unread.

import type { OwnNewsItem } from "./api";

/** The text in this language, else Russian (the reference language), English, or whatever there is. */
export function localized(texts: Record<string, string>, lang: string): string {
  return texts[lang] ?? texts.ru ?? texts.en ?? Object.values(texts)[0] ?? "";
}

/** Entries the player has not seen yet. */
export function unseen(items: OwnNewsItem[], seen: readonly string[]): OwnNewsItem[] {
  return items.filter((item) => !seen.includes(item.id));
}
