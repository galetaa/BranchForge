# Написать contract tests для runtime

## Контекст
Runtime нужно стабилизировать до UI и git логики.

## Цель
Критические protocol regressions ловятся тестами.

## Подзадачи
1. Проверить handshake happy path.
2. Проверить duplicate action rejection.
3. Проверить timeout/invalid response cases.
4. Проверить delivery notifications.

## Зависимости
- T05_request_routing_and_invoke.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Есть набор автоматических contract tests.
- Runtime не требует ручной проверки на каждый PR.
- Есть integration test `hello -> register -> ready` в `plugin_host/tests`.
- Есть contract tests на timeout, invalid response id и notification delivery.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
