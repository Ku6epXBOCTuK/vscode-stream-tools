# Пошаговый перенос

Исходники лежат рядом:

```text
../music-player      vscode-music-player
../twitch-chat       vsc-chat
.                    vscode-stream-tools
```

Старые репозитории не меняются до последнего шага: код копируется, а не
переносится через `git mv`, история остаётся в архиве.

## Карта переноса

| Откуда                                 | Куда                           |
| -------------------------------------- | ------------------------------ |
| `music-player/backend/src/audio.rs`    | `crates/music/src/audio.rs`    |
| `music-player/backend/src/player.rs`   | `crates/music/src/player.rs`   |
| `music-player/backend/src/pump.rs`     | `crates/music/src/pump.rs`     |
| `music-player/backend/src/radio.rs`    | `crates/music/src/radio.rs`    |
| `music-player/backend/src/streamer.rs` | `crates/music/src/streamer.rs` |
| `twitch-chat/backend/src/api.rs`       | `crates/chat/src/api.rs`       |
| `twitch-chat/backend/src/auth.rs`      | `crates/chat/src/auth.rs`      |
| `twitch-chat/backend/src/chat.rs`      | `crates/chat/src/chat.rs`      |
| `twitch-chat/backend/src/clock.rs`     | `crates/chat/src/clock.rs`     |
| `twitch-chat/backend/src/eventsub.rs`  | `crates/chat/src/eventsub.rs`  |
| `twitch-chat/backend/src/sound.rs`     | `crates/chat/src/sound.rs`     |
| `twitch-chat/backend/src/twitch.rs`    | `crates/chat/src/twitch.rs`    |
| оба `backend/src/config.rs`            | `crates/app/src/config.rs`     |
| оба `backend/src/log.rs`               | `crates/app/src/log.rs`        |
| `music-player/backend/src/tray.rs`     | `crates/app/src/tray.rs`       |
| оба `backend/src/main.rs`              | `crates/app/src/main.rs`       |
| `twitch-chat/frontend/`                | `frontend/`                    |
| `twitch-chat/extension/`               | `extension/` (база)            |
| `music-player/extension/`              | `extension/` (команды музыки)  |

Оба `backend/Cargo.toml` схлопываются в `[workspace.dependencies]`: `axum` 0.8,
`reqwest` сводится к одной версии (у чата 0.13, у музыки 0.12), `rodio` нужен
только чату. Два `assets/` выбрасываются, статику собирает `frontend/`.

## Шаги

### 1. Скелет монорепо

```text
Cargo.toml            [workspace] members = crates/*
crates/music/         пока пустая либа
crates/chat/          пока пустая либа
crates/app/           пока пустой bin stream-tools
frontend/
extension/
pnpm-workspace.yaml
justfile
cog.toml
AGENTS.md
README.md
LICENSE
```

`git init`, один начальный коммит, пустой репозиторий на GitHub. Проверка:
`cargo check --workspace` и `pnpm install` проходят на пустых крейтах.

### 2. Крейт `music`

Скопировать пять модулей, убрать `main`, `config`, `log`, `tray` и собственный
Router. Наружу отдать `MusicState`, `router()` (или `handlers`) и
`NowPlayingSource`. Пути становятся `/music/control`, `/music/volume`,
`/music/now_playing`, `/music/playlists[/{index}]`, `/music/stream`. Порт и
сохранение конфига уходят в `app`.

Проверка: `cargo test -p music`, `/music/stream` отдаёт FLAC через тестовый
Router.

### 3. Крейт `chat`

Скопировать семь модулей, убрать `main`, `config`, `log` и собственный Router.
Наружу отдать `ChatState`, `router()` и точки `auth`/`eventsub`. Пути становятся
`/chat/messages[/{id}]`, `/chat/read`, `/chat/moderation/*`, `/chat/channel`,
`/chat/auth/{url,status,callback}`, `/chat/events`, `/chat/eventsub/reconnect`,
`/chat/badges`, `/chat/state`.

Проверка: `cargo test -p chat`, SSE и переподключение EventSub работают через
тестовый Router.

### 4. Крейт `app`

`main`, единый `config.json` с секциями `music`/`chat`/`commands`, один лог
`stream-tools.log` с префиксами `music:` / `chat:`, env `STREAM_TOOLS_LOG`,
трей, один Router: `/api/health`, `/api/shutdown`, `/music/*`, `/chat/*`,
`/panel`, `/overlay`, `/assets/*`, `/` → `/panel`. Порт `45890`. Локи —
`std::sync::nonpoison::{Mutex, RwLock}`.

Проверка: `just backend`, `/api/health` показывает обе секции, `/api/shutdown`
завершает процесс, `config.json` пишется атомарно.

### 5. Frontend

Перенести `twitch-chat/frontend` целиком, `outDir` → `crates/app/assets/app`.
Добавить `panel/components/Music.svelte` и `MusicStrip` под `StatusBar`,
сворачиваемые секции плейлиста и громкости. Удалить `music.html` и любые остатки
отдельной страницы музыки.

Проверка: `pnpm --filter ./frontend run test`, панель собирается в
`crates/app/assets/app`, `/overlay` открывается без старого бэкенда.

### 6. Extension

Собрать один `extension/`: `package.json` с именем `stream-tools-extension`,
одним publisher, командами музыки и чата, одной иконкой статус-бара с текущим
треком. Лончер запускает `~/bin/stream-tools/stream-tools.exe` с `config.json`
рядом, ключи настроек сводятся к `stream-tools.port` и путям.

Проверка: `.vsix` ставится, статус-бар фокусирует панель, плей/пауза и следующий
трек работают из палитры команд.

### 7. `!трек`

В `app` передать в `chat` реализацию `NowPlayingSource` (без HTTP). Правила:
только свой канал, свои сообщения пропускаются, кулдаун, шаблон с `{track}`,
алиасы из конфига, лог срабатывания и отказа.

Проверка: юнит-тесты на кулдаун, шаблон и фильтр своих сообщений.

### 8. Сборка, установка, переключение

`just`-рецепты: build, install в `~/bin/stream-tools/`, `ext-build`,
`ext-install`, health, logs. CI и release-workflow отложены до готовности
приложения к распространению (VST-10). Затем вручную:

1. Установить `stream-tools` и новый `.vsix`.
2. Удалить старые расширения и `~/bin/vscode-music-player`,
   `~/bin/twitch-chat-sidebar`.
3. OBS: `http://127.0.0.1:45890/music/stream`.
4. Twitch: redirect URI `http://localhost:45890/chat/auth/callback`.
5. Архивировать `vscode-music-player` и `vsc-chat`, старые проекты в трекере.
6. Проверить ubuntu-раннер: `libasound2-dev` для `rodio`.
