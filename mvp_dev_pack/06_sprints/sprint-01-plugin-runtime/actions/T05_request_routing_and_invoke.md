# Построить request router и action invoke pipeline

## Контекст
Host должен уметь вызвать plugin method и дождаться response.

## Цель
Есть рабочий sync/async маршрут request-response.

## Подзадачи
1. Сделать request id management.
2. Сделать pending request map.
3. Добавить timeout policy.
4. Проложить путь `host.action.invoke`.

## Зависимости
- T04_handshake_and_registration.md

## Артефакты
- изменения в коде
- тесты на заявленный уровень
- обновление документации, если менялся контракт/архитектура

## Acceptance Criteria
- Host может вызвать action у plugin и получить ответ.
- Есть `request id` генерация и pending request map в runtime.
- Timeout policy автоматически очищает просроченные pending requests.
- `action_engine` использует runtime session для маршрута `host.action.invoke`.
- Есть e2e тест host orchestration (`app_host -> action_engine -> plugin_host`) для invoke/response happy path.

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
