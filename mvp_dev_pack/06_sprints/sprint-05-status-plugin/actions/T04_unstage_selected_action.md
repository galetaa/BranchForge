# Реализовать action `index.unstage_selected`

## Контекст
Нужен обратный путь для staged файлов в рамках того же status workflow.

## Цель
Пользователь может убрать выбранные файлы из stage через job `index.unstage_paths`.

## Подзадачи
1. Поддержать action в runtime registration (`index.unstage_selected`).
2. Передавать selected paths в `JobRequest.paths`.
3. Выполнять `index.unstage_paths` через `execute_job_op`.
4. Обеспечить post-op refresh и корректный fallback для репозиториев без коммитов.

## Зависимости
- `mvp_dev_pack/06_sprints/sprint-05-status-plugin/plugin/T02_selection_flow_from_lists.md`
- `mvp_dev_pack/06_sprints/sprint-05-status-plugin/actions/T03_stage_selected_action.md`

## Артефакты
- `crates/job_system/src/lib.rs` (`execute_job_op`, ветка `index.unstage_paths`)
- `crates/git_service/src/lib.rs` (`unstage_paths`, fallback `git rm --cached`)
- `crates/app_host/tests/open_repo_flow_smoke.rs` (`status_stage_unstage_flow_smoke`)

## Acceptance Criteria
- После unstage файл покидает `staged` и отражается в рабочем дереве (`unstaged`/`untracked`).
- Обновление UI/state происходит автоматически после job completion.

## Verification commands
```bash
cargo test -p git_service stage_then_unstage_moves_file_between_groups
cargo test -p app_host status_stage_unstage_flow_smoke
```

## Definition of Done напоминание
- код собран
- тесты проходят
- документация синхронизирована
- сценарий задачи воспроизводим
