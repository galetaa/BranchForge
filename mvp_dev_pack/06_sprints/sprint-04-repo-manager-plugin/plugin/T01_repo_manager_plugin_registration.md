# Реализовать repo_manager plugin и его регистрацию

## Контекст
Нужен первый реальный bundled plugin, работающий на runtime.

## Цель
Repo plugin реально участвует в runtime, а не существует только как схема.

## Подзадачи
1. Сделать бинарь plugin.
2. Реализовать `plugin.hello/register/ready`.
3. Зарегистрировать action `repo.open`.
4. Подключить логирование plugin stderr/stdout behavior.

## Зависимости
- ../sprint-01-plugin-runtime/plugin-host/T04_handshake_and_registration.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Plugin регистрируется автоматически при запуске host.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
