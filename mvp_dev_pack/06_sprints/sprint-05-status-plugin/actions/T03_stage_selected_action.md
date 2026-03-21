# Реализовать action `index.stage_selected`

## Контекст
Это первая mutating операция из основного status workflow.

## Цель
Пользователь может добавить выбранные файлы в stage через job `index.stage_paths`.

## Подзадачи
1. Поддержать action в runtime registration (`index.stage_selected`).
2. Брать paths из selection state и передавать их в `JobRequest.paths`.
3. Выполнять `index.stage_paths` через `execute_job_op`.
4. После stage делать реактивный `status_refresh` (внутри job execution path).

## Зависимости
- `mvp_dev_pack/06_sprints/sprint-05-status-plugin/plugin/T02_selection_flow_from_lists.md`
- `mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/jobs/T05_job_results_and_state_refresh.md`

## Артефакты
- `crates/job_system/src/lib.rs` (`execute_job_op`, ветка `index.stage_paths`)
- `crates/git_service/src/lib.rs` (`stage_paths`, `status_refresh`)
- `crates/app_host/src/lib.rs` (`run_status_stage_unstage_smoke`)

## Acceptance Criteria
- После stage выбранный файл появляется в `staged`.
- Снимок state обновляется без перезапуска host/view.

## Verification commands
```bash
cargo test -p git_service stage_then_unstage_moves_file_between_groups
cargo test -p job_system execute_stage_and_unstage_paths_updates_status_groups
```

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
