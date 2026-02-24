# ACP-адаптер для Codex (CAS)

Используйте [Codex](https://github.com/openai/codex) из [ACP-совместимых](https://agentclientprotocol.com) клиентов (например, [Zed](https://zed.dev)) через CAS-реализацию.
Проект вдохновлен codex-acp, но реализует собственный CAS-мост и UX-поведение.

## Статус проекта

- Линейка версий CAS начинается с `0.1.0`.
- На стадии `0.x` проект считается бета-версией: возможны изменения поведения и API между релизами.
- Цель этой ветки: стабильный UX для Zed + аккуратный мост между ACP и Codex app-server.
- Текущий практический тестовый контур: в основном сценарий с ChatGPT-подпиской через локальный Codex CLI.
- Авторизация через `OPENAI_API_KEY` и `CODEX_API_KEY` поддерживается в коде, но в этой ветке не проходила отдельную полную валидацию в Zed/CAS-потоке.

## Что поддерживается

- Контекст через `@`-упоминания
- Изображения
- Tool calls с запросами разрешений
- Список сессий и восстановление (`/threads`, `/resume <thread_id>`)
- Replay истории при загрузке/возобновлении
- Настройка модели и reasoning effort
- Обновления использования контекста (tokens/window)
- Команды: `/compact`, `/undo`, `/reasoning`, `/plan`, `/context`

Ограничение: MCP-серверы клиента пока принимаются на уровне ACP, но не пробрасываются в режим Codex app-server.

Примечание по сессиям: история `/threads` и `/resume` берется из локального `CODEX_HOME` (обычно `~/.codex`), а не из отдельного облачного хранилища.

## Быстрый запуск

```bash
OPENAI_API_KEY=sk-... codex-acp
```

## Локальный CAS workflow

- Быстрые проверки: `bash script/run_live_checks.sh quick`
- Полные проверки: `bash script/run_live_checks.sh full`
- Сборка+установка+smoke-test: `bash script/build_install_cas.sh`
- Отдельный smoke-test: `bash script/smoke_test_cas.sh "$HOME/.local/bin/codex-acp-cas"`

## Обновление references

- Ручное обновление всех reference-репозиториев:
  `bash script/update_references.sh`
- Обновление один раз в день (если уже обновлялось сегодня по UTC, скрипт завершится без действий):
  `bash script/update_references.sh --daily`
- Обновление одного reference-репо:
  `bash script/update_references.sh --repo zed`

Скрипт подтягивает изменения из `origin`, определяет версию (`tag`/`describe`/commit) и переименовывает папку в формат `<имя>@<версия>`, например `codex-acp-upstream@v0.9.4`. Для удобства он оставляет стабильный симлинк без версии: `references/codex-acp-upstream -> references/codex-acp-upstream@v0.9.4`.

Пример cron (ежедневно в 04:20 UTC):
`20 4 * * * cd /home/qweasd123tg/Code/zed\ codex\ app\ server && bash script/update_references.sh --daily >> /tmp/cas-update-references.log 2>&1`

## Релизы и версии

Проект использует **независимую SemVer-схему CAS** (не привязанную к тегам upstream), например `0.1.0`, `0.1.1`.
Официальная поддерживаемая платформа релизов: **Fedora x86_64**.
Остальные платформы считаются best-effort: можно собрать самостоятельно, но сопровождение и фиксы не гарантируются.

Подготовка релиза:

```bash
bash script/prepare_release.sh 0.1.0
git push origin main
git push origin v0.1.0
```

Рекомендуется пушить именно нужный релизный тег (`vX.Y.Z`) и не использовать `git push --tags`, чтобы не отправлять лишние теги из локального репозитория.

GitHub Actions релизного контура собирает **только Linux GNU x86_64** (`x86_64-unknown-linux-gnu`) для Fedora-сценария.

## Лицензия

Apache-2.0 (`LICENSE`).
