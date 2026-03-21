# Реализовать typed state store

## Контекст
Store должен хранить snapshots и версии состояний.

## Цель
Есть централизованный state owner.

## Подзадачи
1. Добавить RepoSnapshot/StatusSnapshot/SelectionState.
2. Добавить version counter.
3. Добавить read/update API.
4. Сделать подписки на обновления.

## Зависимости
- нет

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- State store покрыт unit tests.
- Snapshots типизированы.
- Есть `version` counter и подписки на `StateUpdated`.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
