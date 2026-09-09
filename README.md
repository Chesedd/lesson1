# Учебное desktop-приложение

Локальный vertical slice урока для 7–8 классов «Работа с данными в Python. Первое знакомство с pandas». Tauri 2 запускает React UI в отдельном окне, Rust валидирует bundled content и читает доверенный прогресс из локальной SQLite. Интернет, сервер, Docker и PostgreSQL для работы приложения не нужны.

## Требования для Windows

- Node.js 22+ и npm 10+;
- стабильный Rust toolchain с MSVC target (`rustup default stable-msvc`);
- Microsoft C++ Build Tools и Windows 10/11 SDK;
- WebView2 Runtime (в актуальных Windows 10/11 обычно уже установлен).

Полный актуальный перечень platform prerequisites приведён в документации Tauri 2. Архитектура переносима, но первый поддерживаемый target — Windows.

## Запуск

```bash
cd frontend
npm ci
npm run tauri dev
```

Tauri запускает Vite автоматически и открывает урок в desktop window. FastAPI в фоне не запускается.

## Production build

```bash
cd frontend
npm run tauri build
```

Manifest и CSV включаются в resources. SQLite `progress.sqlite3` автоматически создаётся в стандартном application data directory (`ru.lesson1.desktop`), независимо от рабочей директории и расположения executable.

## Проверки

```bash
cd frontend && npm test
cd frontend && npm run lint
cd frontend && npm run build
cargo test --manifest-path frontend/src-tauri/Cargo.toml
cargo check --manifest-path frontend/src-tauri/Cargo.toml
```

## Структура и границы

- `frontend/` — сохранённый React/TypeScript lesson UI и mockable service boundary над Tauri IPC.
- `frontend/src-tauri/` — Tauri shell, Rust validation/application layer и SQLite repository.
- `content/` — version-controlled публичный manifest и CSV, попадающие в bundle.
- `docs/adr/` — история архитектурных решений; web baseline в ADR 0001 заменён ADR 0002.

Доступные UI команды только читают урок и progress. Команд произвольного SQL или ручного completion нет. Python execution, Run, Check и graders намеренно ещё не реализованы.
