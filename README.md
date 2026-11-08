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

## Сборка

```powershell
pnpm install
just backend-build
just check
```

Бинарник `stream-tools.exe` ставится в `~/bin/stream-tools/` вместе с
`config.json` и `assets/`.

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
