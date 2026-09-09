# Архитектурный аудит учебной платформы

Дата аудита: 2026-09-09.

## A. Existing architecture

Репозиторий находится в исходном состоянии: единственный отслеживаемый файл — пустой
`.gitkeep`. В нём нет исходного кода, конфигурации приложения, схемы базы данных,
миграций, API, авторизации, UI-компонентов, редактора кода, Python runner или тестов.
Поэтому существующих frontend/backend-стеков, архитектурных соглашений и механизмов,
которые можно переиспользовать, определить нельзя.

Это важное ограничение аудита: рекомендации ниже являются минимальным стартовым
проектированием, а не интеграцией с уже существующим приложением. До появления
продуктовых ограничений не следует одновременно создавать несколько сервисов или
сложную событийную архитектуру.

## B. Gap analysis

### Уже есть и можно переиспользовать

- Git-репозиторий и текущая ветка разработки.
- Прикладных модулей и компонентов для переиспользования нет.

### Есть, но нужно расширить

- Эта категория пуста: в репозитории нет частичной реализации домена, UI или runner.

### Нужно создать

- Один web frontend с экраном урока, теорией, переключателем задач, простым редактором,
  отдельными действиями **Run** и **Check**, результатами и progress bar.
- Один backend API со строгими request/response-схемами, каталогом учебного контента,
  серверным вычислением прогресса и адаптером изолированного runner.
- Минимальную идентификацию ученика. Для постоянного прогресса нужен стабильный
  `user_id`; если полноценная авторизация ещё не выбрана, MVP может использовать
  выданную сервером анонимную сессию, но не идентификатор из тела запроса.
- Реляционное хранилище фактов прохождения и миграции.
- Отдельную изолированную границу исполнения Python/pandas.
- Unit-, API-, integration- и frontend component-тесты.
- CI-проверки форматирования, типов, тестов и миграций.

### Не стоит делать в первой версии

- Конструктор курсов и CMS, хранение всей учебной разметки в БД.
- Achievements, streaks, leaderboard, сложные unlock rules и процент за теорию.
- Jupyter-совместимую IDE, debugger, language server и совместное редактирование.
- Очередь заданий и микросервисную декомпозицию до подтверждения нагрузки. При этом
  изоляция процесса исполнения обязательна даже в MVP.
- Произвольную установку пакетов, сетевой доступ из sandbox и пользовательские файлы.
- Детальную аналитику попыток и хранение каждого Run, пока это не требуется продукту.

## C. Proposed architecture

Из-за отсутствия стека рекомендуемый старт — небольшой TypeScript web/API monorepo и
отдельный образ runner на Python. Конкретный framework следует зафиксировать отдельным
коротким ADR после уточнения инфраструктуры команды. Практичный baseline: React-based
web framework с server routes, runtime validation схем и PostgreSQL. Это оставляет один
web deployment, но не запускает недоверенный Python в web-процессе.

Предлагаемая структура (все пути предполагаемые):

```text
apps/web/
  src/app/lessons/[lessonId]/       # route экрана урока
  src/features/lesson/              # theory, navigation, progress, exercise UI
  src/features/code-editor/         # textarea/лёгкий editor adapter
  src/server/api/                    # типизированные HTTP handlers
  src/server/progress/               # trusted progress service/repository
packages/contracts/                  # публичные DTO и runtime schemas
packages/content/
  lessons/<lesson-id>/lesson.yaml    # публичный version-controlled контент
  lessons/<lesson-id>/data/*.csv     # разрешённые учебные данные
runner/
  app/                               # внутренний execution protocol
  tests/<exercise-id>/               # закрытые grader definitions
  Dockerfile                         # pinned Python+pandas environment
db/migrations/                       # user/exercise completion tables
tests/                               # API/integration/e2e
```

Скрытые тесты не должны попадать в `packages/content`, публичный API или frontend
bundle. В production runner image они доставляются отдельно от публичного контента.

### End-to-end flow

1. `GET /api/lessons/:lessonId` читает и валидирует опубликованный manifest, исключает
   grader references и возвращает блоки, теорию, публичные поля задач и вычисленный
   сервером progress текущего пользователя.
2. Экран показывает заголовок/progress постоянно, а внутри блока — вкладку теории и
   задачи 1–3. Выбранные блок/задача можно хранить в URL; server state не нужно
   дублировать в глобальном client store.
3. Редактор и несохранённый код — локальное состояние компонента. При открытии задачи
   используется последний код (если MVP его хранит), иначе `starterCode`.
4. `POST /api/exercises/:id/run` принимает только `code`; backend по `exerciseId`
   определяет разрешённые data assets и профиль среды и отправляет job в runner.
5. `POST /api/exercises/:id/check` принимает только `code`; backend сам находит grader,
   запускает обязательные скрытые тесты и интерпретирует структурированный результат.
6. Только успешный **Check** вызывает idempotent upsert факта completion в одной
   серверной транзакции. **Run** никогда не меняет completion.
7. Check response содержит безопасные результаты тестов и заново вычисленный
   `lessonProgress`; frontend заменяет progress ответом сервера и предлагает следующую
   задачу. Повторный успешный Check оставляет число выполненных неизменным.

Минимальные endpoints:

```text
GET  /api/lessons/{lessonId}
POST /api/exercises/{exerciseId}/run
POST /api/exercises/{exerciseId}/check
GET  /api/lessons/{lessonId}/progress   # полезен для refresh, необязателен при первом GET
```

Все ответы имеют discriminated status (`completed`, `failed`, `runtime_error`,
`timeout`, `internal_error`), limits metadata и request/job id. Клиент не передаёт
`passed`, `completed`, ожидаемые значения, grader id или процент.

## D. Data model

### Version-controlled public content

Для первых двух уроков предпочтительнее YAML/JSON manifests: они проходят code review,
версионируются вместе с CSV, легко тестируются схемой и не требуют CMS. Markdown можно
использовать для теории, если renderer имеет строгую sanitization policy.

```ts
type LearningTrack = {
  id: string;
  title: string;
  ageGroup: "grades-7-8" | "grades-9-10";
  lessonIds: string[];
};

type Lesson = {
  id: string;
  version: number;
  trackId: string;
  title: string;
  blocks: LearningBlock[];
};

type LearningBlock = {
  id: string;
  title: string;
  theory: { format: "markdown"; body: string };
  exercises: PublicExercise[]; // MVP invariant: 3, ordered by difficulty
};

type PublicExercise = {
  id: string;
  title: string;
  difficulty: 1 | 2 | 3;
  statement: string;
  starterCode: string;
  dataFiles: Array<{ logicalName: string; assetId: string }>;
  examples?: Array<{ input?: unknown; description: string; output?: unknown }>;
  hints: string[];
  checkerKind: "function" | "variables" | "dataframe" | "stdout";
};
```

`checkerKind` публично объясняет ожидаемый контракт, но не раскрывает ответы. В manifest
можно хранить opaque `graderKey`, который удаляется DTO serializer. Лучше, чтобы runner
разрешал тестовый пакет по `(exerciseId, contentVersion)`, а не принимал тестовый код
от browser.

### Private grader model

```ts
type ExerciseTest = {
  id: string;                 // внутренний стабильный id
  exerciseId: string;
  contentVersion: number;
  titleForStudent: string;    // безопасное имя проверки
  required: boolean;
  timeoutMs?: number;         // не выше server policy
  fixtureAssetIds: string[];
  assertion: PrivateAssertionDefinition;
};
```

Private assertions могут импортировать функцию ученика, проверять значения переменных,
типы/форму/значения DataFrame и свойства результата. stdout — лишь один из видов
наблюдаемого поведения, а не универсальный checker.

### Trusted persistence

Контент остаётся в manifests, а пользовательские факты — в PostgreSQL:

```text
exercise_completion
  user_id             FK / stable session subject
  exercise_id         text
  content_version     integer
  completed_at        timestamptz
  successful_check_id uuid
  PRIMARY KEY (user_id, exercise_id, content_version)

exercise_attempt                  # опционально в MVP, полезно для last code
  id, user_id, exercise_id, content_version
  kind run|check, status, created_at, duration_ms
  code_or_encrypted_blob?, safe_result_json?
```

MVP требует только `exercise_completion` и связь с доверенным пользователем. Attempt
нужен лишь если сразу требуются число попыток, последний код или аудит. Код школьника
содержит персональные данные/секреты, поэтому срок хранения и доступ следует определить
до включения `code_or_encrypted_blob`.

Процент не хранится: completion row — источник истины. Уникальный ключ делает успешный
повтор идемпотентным. `content_version` явно задаёт политику изменений: существенное
изменение тестов создаёт новую версию; косметическое изменение не сбрасывает прогресс.

## E. Python execution and autograding

### Boundary and isolation

- Backend передаёт runner job: случайный id, mode, code, exercise id/version, список
  разрешённых asset ids и жёсткие лимиты. Hidden test source не проходит через browser.
- Каждый job запускается в новом непривилегированном process/container sandbox: без
  host mounts, Docker socket, outbound network и новых privileges; read-only rootfs,
  отдельный writable tmpfs, seccomp/AppArmor, PID/user namespaces и cgroup limits.
- Ограничения включают wall-clock и CPU timeout, память, процессы/потоки, размер файлов,
  stdout/stderr и общий размер результата. По timeout весь process group уничтожается.
- Python и pandas фиксируются digest/version lockfile в runner image. Импорты можно
  ограничить allowlist (`pandas` и стандартные безопасные модули), но AST/import
  filtering — дополнительная защита, не замена OS/container isolation.
- CSV выбирает сервер по `assetId`, копирует read-only под безопасным логическим именем
  в рабочий каталог job и не принимает произвольный путь/URL от клиента.

Вызов локального `exec()` в основном backend-процессе запрещён. Даже отдельный subprocess
без sandbox недостаточен: Python позволяет доступ к файловой системе, процессам и сети.
Если инфраструктура не допускает надёжные short-lived containers, нужен внешний
изолированный execution provider/service до выпуска функции.

### Run versus Check

**Run** выполняет только ученический файл и доступные fixtures. Возвращает ограниченные
`stdout`, `stderr`, exit status, duration и классифицированную ошибку. Он предназначен
для эксперимента и никогда не создаёт completion.

**Check** в том же pinned sandbox сначала загружает решение как отдельный module, затем
запускает private grader adapter. Grader вызывает функции с несколькими inputs,
проверяет переменные или pandas objects (`shape`, columns, values/dtypes с допустимыми
погрешностями) и возвращает JSON через отдельный служебный канал. Обязательные тесты
должны пройти все.

Публичный результат каждого теста содержит только `testId`, безопасный title, status и
педагогическое сообщение. Traceback очищается от путей и grader frames. Expected values,
fixtures и исходник assertion не возвращаются. Internal errors логируются по job id, а
ученику показывается нейтральное сообщение. Результату runner доверяет только backend по
аутентифицированному внутреннему каналу; backend проверяет schema и соответствие job.

## F. Lesson progress

1. Источник истины — `exercise_completion`, записанный progress service только после
   валидного результата обязательных hidden tests.
2. Сервис загружает текущую опубликованную версию lesson manifest и множество её
   обязательных exercise ids. `total = count(required exercises)`; сейчас все 12/15
   обязательны. `completed` — пересечение с completion rows пользователя этой версии.
3. `percent = total === 0 ? 0 : floor(completed * 100 / total)` (или единообразное
   округление, зафиксированное контрактным тестом). DTO также содержит `remaining`.
4. После успешного Check backend в транзакции делает `INSERT ... ON CONFLICT DO NOTHING`,
   затем считает progress и возвращает его. Поэтому повторный успех не прибавляет задачу.
5. Frontend считает ответ сервера authoritative, обновляет query cache/state и progress
   bar. При reload данные приходят через lesson GET; optimistic completion запрещён.
6. При добавлении задачи `total` немедленно меняется по manifest и процент может
   уменьшиться. Для опубликованных уроков безопаснее повышать `contentVersion` и явно
   выбрать: сохранять старое прохождение либо начать новую ревизию. Нельзя молча менять
   semantic ids или тесты.
7. Клиент может визуально показывать pending Check, но только server response меняет
   `completedExerciseIds`. Подмена процента в DevTools не меняет persisted fact.

DTO:

```ts
type LessonProgress = {
  lessonId: string;
  contentVersion: number;
  completed: number;
  total: number;
  remaining: number;
  percent: number;
  completedExerciseIds: string[];
};
```

Позже без изменения источника истины можно добавить агрегаты по block/course, optional
exercises и theory status. Их не нужно включать в MVP.

## G. Implementation phases

### 1. Зафиксировать baseline и contracts

- **Цель:** выбрать поддерживаемый командой web/API stack и определить публичные схемы.
- **Файлы:** ADR в `docs/adr/`, workspace manifests, `packages/contracts`, content schema.
- **Backend:** skeleton health endpoint и schema validation.
- **Frontend:** skeleton lesson route без runner.
- **Tests:** contract/schema tests и CI smoke check.
- **Готово:** invalid lesson manifest не собирается; web/API запускаются одной командой.

### 2. Каталог и вертикальный read-only урок

- **Цель:** один полностью описанный блок с теорией и тремя задачами.
- **Файлы:** `packages/content/lessons/...`, `apps/web/src/features/lesson/...`.
- **Backend:** public serializer, не пропускающий private fields.
- **Frontend:** progress header, theory/task navigation, editor placeholder.
- **Tests:** manifest, serializer snapshot/contract, accessibility/component tests.
- **Готово:** пользователь открывает урок; hidden metadata отсутствует в HTML/API.

### 3. Изолированный Run

- **Цель:** безопасно исполнять Python и pandas с одним CSV.
- **Файлы:** `runner/`, run handler/client, deployment sandbox policy.
- **Backend:** limits, job protocol, stdout/stderr normalization.
- **Frontend:** редактор, Run, output/error panel и loading/cancel UX.
- **Tests:** success, syntax/runtime error, infinite loop, memory/process/network/file
  escape, oversized output, pandas/CSV integration.
- **Готово:** adversarial jobs завершаются лимитами и не видят host/network.

### 4. Hidden Check

- **Цель:** проверять observable behavior, не раскрывая graders.
- **Файлы:** private grader packages, check handler и test result UI.
- **Backend:** server-selected grader, signed/internal job, safe result mapper.
- **Frontend:** Check и список безопасных test outcomes; Run остаётся отдельным.
- **Tests:** function/variable/DataFrame checkers, partial failure, leakage tests.
- **Готово:** только все required tests дают trusted `passed=true`; test source не
  присутствует в client assets/responses.

### 5. Persistent lesson progress

- **Цель:** надёжно сохранить completion и показать progress.
- **Файлы:** `db/migrations`, progress repository/service, lesson/check DTO and UI.
- **Backend:** authenticated subject, idempotent upsert, server aggregation.
- **Frontend:** обновление cache из Check response и восстановление после reload.
- **Tests:** first/repeated/concurrent success, failed Check, cross-user isolation,
  6/12=50%, 9/15=60%, manifest version change.
- **Готово:** Run/failed Check не меняют progress; повторный success не увеличивает его.

### 6. Заполнить два урока и провести hardening

- **Цель:** 4×3 и 5×3 задачи, доступность и production readiness.
- **Файлы:** manifests/Markdown/CSV/private graders, e2e and operations docs.
- **Backend:** observability, rate limits, retention and runner capacity controls.
- **Frontend:** responsive layout, понятные русские ошибки, next-task navigation.
- **Tests:** все 27 reference solutions, intentional failures, end-to-end user journeys,
  security regression and accessibility checks.
- **Готово:** 12 и 15 заданий проходят эталонами, ошибочные решения отклоняются,
  progress соответствует серверным completion facts.

## H. Risks / open questions

- **Нет исходного приложения:** стек, deployment platform, UI conventions, identity и
  эксплуатационные ограничения нельзя вывести из репозитория. Выбор до уточнения может
  создать именно ту параллельную архитектуру, которую требуется избежать.
- **Identity:** без стабильной аутентификации нельзя безопасно сохранять межсессионный
  прогресс или обеспечить разделение учеников. Нужно решить, есть ли школьный SSO,
  teacher-issued accounts или допустима анонимная cookie session.
- **Sandbox availability:** безопасность зависит от orchestration/runtime (Linux
  namespaces/cgroups, managed sandbox). Docker daemon рядом с web app и особенно его
  socket создают существенный privilege risk.
- **Детская аудитория и данные:** нужны решения о возрасте, согласии, retention,
  доступе преподавателя и хранении исходного кода до реализации Attempts.
- **Политика content version:** продукт должен решить, сохраняется ли completion при
  существенной замене задачи; иначе изменение 12/15 заданий даст неожиданный прогресс.
- **Нагрузка:** одновременный старт класса создаёт burst runner jobs; необходимы
  concurrency quotas и понятное состояние queue/busy, даже если очередь не нужна сразу.
- **Качество graders:** проверки DataFrame должны учитывать порядок, dtype, NaN и
  floating-point tolerance; слишком точные тесты будут отвергать корректные решения.

## I. Recommended next step

Следующий единственный этап — **Phase 1: baseline и contracts**. До написания runner или
27 задач следует коротко подтвердить deployment platform, поддерживаемый командой web
stack и способ идентификации ученика, затем оформить ADR и реализовать валидируемую
публичную схему manifest/DTO с одним демонстрационным блоком. Это даст проверяемый
контракт контента без преждевременной привязки UI, progress и sandbox к неподтверждённым
технологиям.
