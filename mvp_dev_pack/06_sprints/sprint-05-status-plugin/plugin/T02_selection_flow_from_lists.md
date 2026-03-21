# Реализовать selection flow из list events

## Контекст
Без selection stage/unstage actions не имеют контекста.

## Цель
Выбор файлов становится частью host state и используется action-путями `index.stage_selected` / `index.unstage_selected`.

## Подзадачи
1. Обновлять selection через `StateStore::update_selection`.
2. Пробрасывать выбранные пути в action/job path (`index.stage_paths` / `index.unstage_paths`).
3. Проверить, что multi-select сохраняется в `SelectionState`.
4. Подтвердить сценарий в smoke-потоке `select -> stage -> unstage`.

## Зависимости
- `T01_status_plugin_registration_and_view.md`
- `mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/state/T03_state_models_and_store.md`

## Артефакты
- `crates/state_store/src/lib.rs` (`SelectionState`, `update_selection`)
- `crates/app_host/src/lib.rs` (`run_selection_event_smoke`, `run_status_stage_unstage_smoke`)
- `crates/app_host/tests/open_repo_flow_smoke.rs` (`status_stage_unstage_flow_smoke`)

## Acceptance Criteria
- Selection обновляется и сохраняется в state.
- Selection используется как источник путей для stage/unstage job path.

## Verification commands
```bash
cargo test -p app_host selection_event_smoke_updates_selection_state
cargo test -p app_host status_stage_unstage_flow_smoke
```

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
