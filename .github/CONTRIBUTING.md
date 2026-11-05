# Участие в проекте / Contributing

*English below.*

1. Для крупной правки сначала откройте Issue: так проще договориться о подходе.
2. Прогоните проверки из раздела «Сборка из исходников» в [README](../README.md).
3. Любое изменение, заметное пользователю, запишите в `## [Не выпущено]` в [CHANGELOG.md](../CHANGELOG.md) в том же коммите. Правила: [docs/CHANGELOG_RULES.md](../docs/CHANGELOG_RULES.md).
4. Строки интерфейса добавляйте через `t("ключ")` во все файлы `app/src/lib/locales`. Эталонный язык — русский.
5. Цвета берите из переменных темы: так правка будет работать во всех темах. Подробнее: [docs/design-handoff.md](../docs/design-handoff.md).

---

1. For a large change, open an Issue first so we can agree on the approach.
2. Run the checks from "Building from source" in the [README](../README.en.md).
3. Record any user-visible change under `## [Не выпущено]` in [CHANGELOG.md](../CHANGELOG.md) in the same commit. Rules: [docs/CHANGELOG_RULES.md](../docs/CHANGELOG_RULES.md).
4. Add interface strings through `t("key")` to every file in `app/src/lib/locales`. Russian is the reference language.
5. Take colors from the theme variables so the change works in every theme. More: [docs/design-handoff.md](../docs/design-handoff.md).
