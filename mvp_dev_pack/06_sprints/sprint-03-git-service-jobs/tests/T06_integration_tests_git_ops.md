# Добавить integration tests на git ops v0.1

## Контекст
Без e2e-проверки git слоя высокий риск скрытых регрессий.

## Цель
Git service можно менять безопаснее.

## Подзадачи
1. Создавать temp git repo.
2. Тестировать repo.open.
3. Тестировать status.refresh.
4. Подготовить foundation для stage/commit tests.

## Зависимости
- T03_repo_open_and_status_handlers.md
- T05_job_results_and_state_refresh.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Integration tests проходят локально и в CI.
- Temp-repo integration tests покрывают `repo.open` и `status.refresh` foundation path.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
