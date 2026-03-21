# Связать store и event bus

## Контекст
Plugins должны узнавать, что состояние изменилось.

## Цель
Host может уведомлять plugins об обновлениях store.

## Подзадачи
1. Сделать publish `event.state.updated`.
2. Сделать publish `event.repo.opened`.
3. Подвязать event delivery к plugin subscriptions.
4. Добавить smoke тест на событие обновления.

## Зависимости
- T03_state_models_and_store.md
- ../sprint-01-plugin-runtime/tests/T06_contract_tests_runtime.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Подписанный plugin получает event после изменения store.
- `event.repo.opened` и `event.state.updated` проходят через runtime subscriptions в smoke-тесте.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
