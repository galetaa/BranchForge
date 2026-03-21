# Реализовать command palette

## Контекст
Palette — главный вход в actions MVP.

## Цель
Пользователь может вызывать plugin actions без дополнительного UI.

## Подзадачи
1. Показать список actions из registry.
2. Фильтрация по title.
3. Оценка `when`.
4. Invoke выбранного action.

## Зависимости
- T01_window_layout_and_slots.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Palette открывается и исполняет action.
- Host-side palette поддерживает фильтрацию по title и `when` (`always`, `repo.is_open`).

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
