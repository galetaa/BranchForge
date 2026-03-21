# Зарегистрировать status plugin и view `status.panel`

## Контекст
Нужен второй реальный плагин, который работает с state и UI.

## Цель
Host отображает status panel после открытия repo.

## Подзадачи
1. Сделать бинарь status plugin.
2. Зарегистрировать view `status.panel`.
3. Подписаться на repo/state/job events.
4. Реализовать `host.view.get_model`.

## Зависимости
- ../sprint-01-plugin-runtime/plugin-host/T04_handshake_and_registration.md
- ../sprint-02-ui-shell-state/viewmodel/T05_viewmodel_renderer_v0_1.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Status panel появляется только при открытом repo.
- Runtime registration включает view `status.panel` и actions `index.stage_selected`/`index.unstage_selected`.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
