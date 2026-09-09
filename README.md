# Учебное приложение: первый вертикальный срез

Минимальное приложение показывает version-controlled урок pandas, создаёт анонимную learner-session и вычисляет прогресс по доверенным записям PostgreSQL. Выполнение пользовательского кода и проверка решений намеренно не входят в этот этап.

## Требования

- Python 3.12+, Node.js 22+ и npm 10+
- Docker с Compose

## Структура

- `backend/` — синхронный FastAPI, SQLAlchemy 2, Alembic и pytest.
- `frontend/` — React, TypeScript, Vite, Vitest и Testing Library.
- `content/` — строго валидируемый JSON manifest и учебные assets.
- `docs/adr/` — архитектурные решения.

## Локальный запуск

```bash
docker compose up -d postgres
cd backend
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.lock
alembic upgrade head
uvicorn app.main:app --reload
```

В другом терминале:

```bash
cd frontend
npm ci
npm run dev
```

Откройте `http://localhost:5173/lessons/pandas-intro`. Vite проксирует `/api` на backend. Для production задайте `COOKIE_SECURE=true`; также настройте `DATABASE_URL`, `CONTENT_MANIFEST_PATH` и `CORS_ORIGIN` при необходимости.

## Миграции и проверки

```bash
cd backend && alembic upgrade head
cd backend && pytest
cd frontend && npm test
cd frontend && npm run lint
cd frontend && npm run build
```
