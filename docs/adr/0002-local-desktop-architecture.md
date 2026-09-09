# ADR 0002: локальная desktop-архитектура

- **Статус:** принято
- **Дата:** 2026-09-09
- **Заменяет:** ADR 0001

## Контекст

Первый vertical slice был браузерным приложением с FastAPI, PostgreSQL и анонимной cookie-session. Целевой продукт теперь должен устанавливаться на Windows и в дальнейшем полностью работать без сети. Полезные UI, содержание урока, стабильные идентификаторы и семантику доверенного прогресса нужно сохранить, не поддерживая два production runtime.

## Решение

- Tauri 2 владеет жизненным циклом desktop-приложения и открывает React/TypeScript/Vite UI в WebView2.
- Rust является доверенной границей: он загружает и строго валидирует bundled manifest, строит только `PublicLesson` и вычисляет progress.
- Version-controlled `content/` включается в resources сборки. Mutable SQLite создаётся в стандартном app data directory, а не в resources или рядом с executable.
- Локальный MVP однопользовательский: одна установка означает один неименованный профиль. Authentication, cookies, PII и случайный learner id отсутствуют. В будущем storage boundary можно расширить local profile id или синхронизацией.
- React вызывает только узкие Tauri commands `get_lesson` и `get_lesson_progress`. Универсального SQL API и production-команды ручного completion нет.
- Completion хранится как факт `(exercise_id, content_version)`; процент всегда считается из текущего manifest. Schema version хранится локально.
- Public content отделён от будущих private graders. Hidden tests никогда не сериализуются в React; будущий изолированный локальный Python runner останется на trusted side.

## Последствия

FastAPI, SQLAlchemy, Alembic, PostgreSQL, Docker Compose и cookie identity удалены из runtime. Приложению нужны desktop build prerequisites, но сеть после установки не нужна для текущего урока. Следующий этап может добавить локальный редактор и безопасный `Run`, не добавляя hidden autograding.
