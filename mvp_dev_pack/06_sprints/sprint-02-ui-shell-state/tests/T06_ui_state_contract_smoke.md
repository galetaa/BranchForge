# Покрыть связку UI-shell и store smoke-тестами

## Контекст
Нужно зафиксировать, что renderer, state и action availability работают вместе.

## Цель
Основной UI слой не ломается незаметно.

## Подзадачи
1. Проверить отображение view после регистрации.
2. Проверить hide/show по `when`.
3. Проверить selection update из list event.
4. Проверить palette integration.

## Зависимости
- T02_command_palette.md
- T05_viewmodel_renderer_v0_1.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Есть smoke suite на связку UI + state + plugin runtime.
- Есть smoke тест `app_host/tests/ui_state_contract_smoke.rs`.
- Smoke suite включает проверку window layout slots.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
