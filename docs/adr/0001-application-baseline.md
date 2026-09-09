# ADR 0001: технологический baseline приложения

- Статус: заменено [ADR 0002](0002-local-desktop-architecture.md)
- Дата: 2026-09-09

## Контекст и решение

Backend реализуется синхронно на FastAPI, Pydantic v2 и SQLAlchemy 2.x: API и строгие DTO остаются небольшими, а единый sync-подход исключает случайное смешение моделей исполнения. PostgreSQL выбран как production-хранилище доверенного прогресса; Alembic версионирует схему.

Frontend строится на React и TypeScript через Vite. Это даёт компактный компонентный экран, быструю разработку и типовую проверку без тяжёлой UI-библиотеки или Redux. Vitest и Testing Library проверяют пользовательское поведение.

Учебный контент хранится в JSON рядом с кодом, проходит строгую Pydantic-валидацию при старте и меняется через review. Клиент получает только public DTO. Assets публикуются как метаданные по stable id: manifest не содержит серверных путей.

Для MVP случайный opaque identifier связывает HttpOnly/SameSite cookie с серверной записью learner без PII. Это временная identity-модель: stable exercise ids и completion остаются пригодными при присоединении полноценного пользователя.

Python runner, Run, Check, sandbox и hidden graders вынесены в следующий security-sensitive этап. Этот срез не исполняет пользовательский код и не предоставляет обходного endpoint для completion.

## Последствия

Прогресс вычисляется из уникальных completion `(learner_id, exercise_id, content_version)`, а его denominator — из текущего manifest. Содержимое и БД требуют согласования stable id и content version при выпуске новой версии.
