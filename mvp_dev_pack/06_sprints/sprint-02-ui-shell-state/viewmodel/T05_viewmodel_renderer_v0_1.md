# Реализовать host-side renderer ViewModel DSL v0.1

## Контекст
Нужен рендер `container/text/list/button` без кастомной логики у plugin.

## Цель
Host способен отображать MVP status panel.

## Подзадачи
1. Определить enum узлов ViewModel.
2. Рендерить дерево в UI.
3. Поддержать `items_ref` для status lists.
4. Поддержать `enabled_when` и `on_action`.

## Зависимости
- T01_window_layout_and_slots.md
- T03_state_models_and_store.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- ViewModel from plugin отображается корректно.
- Кнопки и списки работают.
- Host корректно рендерит status panel с `staged/unstaged/untracked` группами.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
