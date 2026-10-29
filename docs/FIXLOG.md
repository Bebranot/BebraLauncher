# Журнал исправлений по docs/CODE_REVIEW.md

Ревизия базы: `main`, коммит `2e72d6a`, версия `0.3.0`.
Порядок: P0 → P1 → P2 → P3, внутри приоритета — §9 «Порядок исправлений».

| ID | Статус | Коммит | Что проверено |
|---|---|---|---|
| P0-1 | исправлено | `0c2ac8d` | `login_urls` удалён из модели `/info` и из `play::prepare`; тест `play::tests::auth_server_url_ignores_server_supplied_login_urls` падал до правки (URL брался из `/info`) |
| P0-2 | исправлено | `9672e63` | битый `engines/<ver>.zip` заменяется проверенной загрузкой, `.sig` пишется атомарно; тест `engine::tests::broken_installed_zip_is_replaced_with_verified_download` |
| P1-40 | исправлено | `59143ff` | `cargo fmt --all --check` зелёный |
