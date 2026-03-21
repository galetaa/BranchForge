# Реализовать action `repo.open`

## Контекст
Это первый настоящий пользовательский flow поверх protocol, jobs и dialogs.

## Цель
Пользователь может открыть репозиторий из palette.

## Подзадачи
1. Вызвать `host.ui.pick_folder`.
2. Проверить cancellation path.
3. Запустить `repo.open` job.
4. Вернуть accepted/completed статусы корректно.

## Зависимости
- T01_repo_manager_plugin_registration.md
- ../sprint-03-git-service-jobs/git/T03_repo_open_and_status_handlers.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Open repo flow работает end-to-end.
- Cancel path не меняет store и не приводит к ошибке.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
