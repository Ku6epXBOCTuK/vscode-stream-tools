# Stream Tools

Музыкальный плеер и Twitch-чат в одном расширении VS Code и одном локальном
Rust-процессе.

## Структура

```text
crates/music   плеер, плейлисты, радио, FLAC-поток
crates/chat    Twitch auth, EventSub, сообщения, звук
crates/app     бинарник stream-tools: Router, config, log, tray
extension/     один .vsix
frontend/      Svelte: панель и оверлей
```

## Сборка и установка

```powershell
pnpm install
just install
```

`just install` собирает бэкенд и фронтенд, кладёт `stream-tools.exe` и `assets/`
в `~/bin/stream-tools/`. `config.json` создаётся при первом запуске рядом с
бинарником.

```powershell
just health   # /api/health на 45890
just logs     # последние строки stream-tools.log
just stop     # мягко остановить процесс
just ext-package
```

Другой порт для проверок: `just --set port 45998 health`.

## Маршруты

- `/panel`, `/overlay`, `/assets/*`
- `/music/control`, `/music/volume`, `/music/now_playing`, `/music/playlists`,
  `/music/stream`
- `/chat/messages`, `/chat/read`, `/chat/moderation/*`, `/chat/channel`,
  `/chat/auth/*`, `/chat/events`, `/chat/badges`, `/chat/state`
- `/api/health`, `/api/shutdown`

## OBS и Twitch

- Browser Source в OBS: `http://127.0.0.1:45890/overlay`
- Media Source в OBS: `http://127.0.0.1:45890/music/stream`
- Redirect URI в консоли Twitch: `http://localhost:45890/chat/auth/callback`

Подробности — `docs/plan.md` и `docs/migration.md`.

## Лицензия

MIT. Автор: Ku6epXBOCTuK.
