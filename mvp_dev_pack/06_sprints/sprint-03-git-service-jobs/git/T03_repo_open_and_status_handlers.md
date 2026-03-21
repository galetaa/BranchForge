# Реализовать op handlers `repo.open` и `status.refresh`

## Контекст
Это создаёт основной поток открытия репозитория и построения snapshots.

## Цель
Host может открыть repo и собрать минимальное состояние.

## Подзадачи
1. Проверка git repository.
2. Получение top-level path.
3. Определение branch/detached.
4. Запуск status refresh.
5. Обновление RepoSnapshot и StatusSnapshot.

## Зависимости
- T02_status_porcelain_parser.md
- ../sprint-02-ui-shell-state/state/T03_state_models_and_store.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- repo.open работает на реальном тестовом repo.
- status.refresh обновляет StatusSnapshot из porcelain parser.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
