# Реализовать parser `git status --porcelain=v2 -z`

## Контекст
StatusSnapshot зависит от надёжного parser-а.

## Цель
StatusSnapshot строится корректно.

## Подзадачи
1. Разобрать staged/unstaged/untracked.
2. Покрыть rename/unknown cases минимально.
3. Преобразовать output в StatusEntry[] и summary.
4. Добавить fixture tests.

## Зависимости
- T01_git_command_runner.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Parser проходит fixture tests на типичных кейсах.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
