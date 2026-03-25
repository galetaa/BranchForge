use std::collections::{HashMap, VecDeque};
use std::path::Path;

use git_service::{GitCommand, GitServiceError, RepoOpenResult, StatusSummary};
use plugin_api::{ConflictState, RepoSnapshot};
use state_store::{
    BranchInfo, CommitDetails, DiffChunk, DiffDescriptor, DiffLoadRequest, DiffSource, DiffState,
    HistoryCursor, JournalStatus, OperationSessionKind, OperationSessionState, RebaseEntryAction,
    RebasePlan, RebasePlanEntry, RebaseSessionSnapshot, RefSnapshotSummary, SelectionState,
    StateStore, StatusSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobLock {
    Read,
    IndexWrite,
    RefsWrite,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRequest {
    pub op: String,
    pub lock: JobLock,
    pub paths: Vec<String>,
    pub job_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobExecutionResult {
    pub op: String,
    pub success: bool,
    pub state_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobExecutionError {
    UnsupportedOp { op: String },
    InvalidInput { message: String },
    Git(GitServiceError),
}

impl From<GitServiceError> for JobExecutionError {
    fn from(value: GitServiceError) -> Self {
        Self::Git(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub request: JobRequest,
    pub state: JobState,
}

#[derive(Debug, Default)]
pub struct JobQueue {
    next_id: u64,
    queued: VecDeque<Job>,
    running: HashMap<u64, Job>,
}

impl JobQueue {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            queued: VecDeque::new(),
            running: HashMap::new(),
        }
    }

    pub fn enqueue(&mut self, request: JobRequest) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = request;
        request.job_id = Some(id);

        self.queued.push_back(Job {
            id,
            request,
            state: JobState::Queued,
        });
        id
    }

    pub fn try_start_next(&mut self) -> Option<Job> {
        let position = self
            .queued
            .iter()
            .position(|job| self.can_start(job.request.lock));

        let index = position?;
        let maybe_job = self.queued.remove(index);
        if let Some(mut job) = maybe_job {
            job.state = JobState::Running;
            self.running.insert(job.id, job.clone());
            Some(job)
        } else {
            None
        }
    }

    pub fn finish(&mut self, job_id: u64, success: bool) -> Option<Job> {
        let mut job = self.running.remove(&job_id)?;
        job.state = if success {
            JobState::Succeeded
        } else {
            JobState::Failed
        };
        Some(job)
    }

    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    fn can_start(&self, requested: JobLock) -> bool {
        for running_job in self.running.values() {
            if locks_conflict(running_job.request.lock, requested) {
                return false;
            }
        }
        true
    }
}

pub fn locks_conflict(left: JobLock, right: JobLock) -> bool {
    !matches!((left, right), (JobLock::Read, JobLock::Read))
}

pub fn map_to_git_command(op: &str) -> Option<GitCommand> {
    match op {
        "status.refresh" => Some(GitCommand::status_porcelain()),
        _ => None,
    }
}

pub fn execute_job_op(
    cwd: &Path,
    request: &JobRequest,
    store: &mut StateStore,
) -> Result<JobExecutionResult, JobExecutionError> {
    let session_id = if is_session_op(&request.op) {
        Some(store.allocate_session_id())
    } else {
        None
    };
    let pre_refs = if is_ref_snapshot_op(&request.op) {
        capture_ref_snapshot(cwd)
    } else {
        None
    };
    let journal_entry = start_journal_entry(store, request, session_id, pre_refs);
    let result = (|| match request.op.as_str() {
        "repo.open" => {
            refresh_repo_and_status(cwd, store)?;
            refresh_refs(cwd, store)?;
            sync_rebase_session_state(cwd, store)?;
            store.clear_journal();
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "status.refresh" => {
            refresh_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "index.stage_paths" => {
            git_service::stage_paths(cwd, &request.paths)?;
            refresh_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "index.unstage_paths" => {
            git_service::unstage_paths(cwd, &request.paths)?;
            refresh_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "index.stage_hunk" => {
            let path = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "index.stage_hunk requires path in request.paths[0]".to_string(),
                }
            })?;
            let hunk_index = request
                .paths
                .get(1)
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| JobExecutionError::InvalidInput {
                    message: "index.stage_hunk requires hunk index in request.paths[1]".to_string(),
                })?;
            git_service::stage_hunk(cwd, path, hunk_index)?;
            refresh_status(cwd, store)?;
            refresh_loaded_diff(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "index.unstage_hunk" => {
            let path = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "index.unstage_hunk requires path in request.paths[0]".to_string(),
                }
            })?;
            let hunk_index = request
                .paths
                .get(1)
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| JobExecutionError::InvalidInput {
                    message: "index.unstage_hunk requires hunk index in request.paths[1]"
                        .to_string(),
                })?;
            git_service::unstage_hunk(cwd, path, hunk_index)?;
            refresh_status(cwd, store)?;
            refresh_loaded_diff(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "file.discard_hunk" => {
            let path = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "file.discard_hunk requires path in request.paths[0]".to_string(),
                }
            })?;
            let hunk_index = request
                .paths
                .get(1)
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| JobExecutionError::InvalidInput {
                    message: "file.discard_hunk requires hunk index in request.paths[1]"
                        .to_string(),
                })?;
            git_service::discard_hunk(cwd, path, hunk_index)?;
            refresh_status(cwd, store)?;
            refresh_loaded_diff(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "commit.create" => {
            ensure_no_conflicts(cwd, "commit.create")?;
            let message = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "commit.create requires message in request.paths[0]".to_string(),
                }
            })?;

            git_service::commit_create(cwd, message)?;
            refresh_repo_and_status(cwd, store)?;
            refresh_refs(cwd, store)?;

            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "history.page" => {
            let (offset, limit) = parse_history_request(request)?;
            let filter_author = request.paths.get(2).cloned().filter(|v| !v.is_empty());
            let filter_text = request.paths.get(3).cloned().filter(|v| !v.is_empty());
            let hash_prefix = request.paths.get(4).cloned().filter(|v| !v.is_empty());
            load_history_page(
                cwd,
                store,
                HistoryLoadRequest {
                    op: &request.op,
                    offset,
                    limit,
                    append: offset > 0,
                    filter_author,
                    filter_text,
                    hash_prefix,
                },
            )
        }
        "history.load_more" => {
            let cursor = store.snapshot().history.next_cursor.clone();
            let Some(cursor) = cursor else {
                return Ok(JobExecutionResult {
                    op: request.op.clone(),
                    success: true,
                    state_version: store.snapshot().version,
                });
            };
            let filter_author = store.snapshot().history.filter_author.clone();
            let filter_text = store.snapshot().history.filter_text.clone();
            load_history_page(
                cwd,
                store,
                HistoryLoadRequest {
                    op: &request.op,
                    offset: cursor.offset,
                    limit: cursor.page_size,
                    append: true,
                    filter_author,
                    filter_text,
                    hash_prefix: None,
                },
            )
        }
        "refs.refresh" => {
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "history.search" => {
            let (offset, limit) = parse_history_request(request)?;
            let filter_author = request.paths.get(2).cloned().filter(|v| !v.is_empty());
            let filter_text = request.paths.get(3).cloned().filter(|v| !v.is_empty());
            let hash_prefix = request.paths.get(4).cloned().filter(|v| !v.is_empty());
            load_history_page(
                cwd,
                store,
                HistoryLoadRequest {
                    op: &request.op,
                    offset,
                    limit,
                    append: false,
                    filter_author,
                    filter_text,
                    hash_prefix,
                },
            )
        }
        "history.clear_filter" => load_history_page(
            cwd,
            store,
            HistoryLoadRequest {
                op: &request.op,
                offset: 0,
                limit: 20,
                append: false,
                filter_author: None,
                filter_text: None,
                hash_prefix: None,
            },
        ),
        "history.file" => {
            let path = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "history.file requires file path in request.paths[0]".to_string(),
                }
            })?;
            let offset = request
                .paths
                .get(1)
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let limit = request
                .paths
                .get(2)
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(20);
            let commits = git_service::file_history_page(cwd, path, offset, limit)?;
            let next_cursor = if commits.len() == limit {
                Some(HistoryCursor {
                    offset: offset + commits.len(),
                    page_size: limit,
                })
            } else {
                None
            };
            let history_commits = commits
                .into_iter()
                .map(|commit| state_store::CommitSummary {
                    oid: commit.oid,
                    author: commit.author,
                    time: commit.time,
                    summary: commit.summary,
                })
                .collect();
            store.update_history_page(
                history_commits,
                next_cursor,
                false,
                None,
                Some(path.to_string()),
            );
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "blame.file" => {
            let path = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "blame.file requires file path in request.paths[0]".to_string(),
                }
            })?;
            let rev = request.paths.get(1).map(String::as_str);
            let lines = git_service::file_blame(cwd, path, rev)?;
            let text = lines
                .into_iter()
                .map(|line| {
                    let short = line.oid.chars().take(8).collect::<String>();
                    format!(
                        "{} {} {} | {}",
                        line.line_no, short, line.author, line.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            store.update_diff(build_diff_state(
                DiffSource::Commit {
                    oid: format!("blame:{path}"),
                },
                text,
                Vec::new(),
            ));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "stash.create" => {
            let message = request.paths.first().map(String::as_str);
            git_service::stash_create(cwd, message)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "stash.list" => {
            let entries = git_service::stash_list(cwd)?;
            let text = if entries.is_empty() {
                "stash: <empty>".to_string()
            } else {
                entries
                    .into_iter()
                    .map(|entry| format!("{} {}", entry.reference, entry.message))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            store.update_diff(build_diff_state(
                DiffSource::Commit {
                    oid: "stash:list".to_string(),
                },
                text,
                Vec::new(),
            ));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "worktree.list" => {
            let entries = git_service::list_worktrees(cwd)?;
            let text = entries
                .into_iter()
                .map(|entry| {
                    let branch = entry.branch.unwrap_or_else(|| "<detached>".to_string());
                    format!("{} [{}]", entry.path, branch)
                })
                .collect::<Vec<_>>()
                .join("\n");
            store.update_diff(build_diff_state(
                DiffSource::Commit {
                    oid: "worktree:list".to_string(),
                },
                text,
                Vec::new(),
            ));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "worktree.create" => {
            let path = request
                .paths
                .first()
                .ok_or_else(|| JobExecutionError::InvalidInput {
                    message: "worktree.create requires path in request.paths[0]".to_string(),
                })?;
            let branch = request.paths.get(1).map(String::as_str);
            git_service::worktree_add(cwd, std::path::Path::new(path), branch)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "worktree.remove" => {
            let path = request
                .paths
                .first()
                .ok_or_else(|| JobExecutionError::InvalidInput {
                    message: "worktree.remove requires path in request.paths[0]".to_string(),
                })?;
            let force = request.paths.get(1).map(|v| v == "force").unwrap_or(false);
            git_service::worktree_remove(cwd, std::path::Path::new(path), force)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "worktree.open" => {
            let path = request
                .paths
                .first()
                .ok_or_else(|| JobExecutionError::InvalidInput {
                    message: "worktree.open requires path in request.paths[0]".to_string(),
                })?;
            refresh_repo_and_status_for_path(std::path::Path::new(path), store)?;
            refresh_refs(std::path::Path::new(path), store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "submodule.list" => {
            let entries = git_service::list_submodules(cwd)?;
            let text = entries
                .into_iter()
                .map(|entry| format!("{} {} {}", entry.status, entry.oid, entry.path))
                .collect::<Vec<_>>()
                .join("\n");
            store.update_diff(build_diff_state(
                DiffSource::Commit {
                    oid: "submodule:list".to_string(),
                },
                text,
                Vec::new(),
            ));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "submodule.init_update" => {
            let path = request.paths.first().map(String::as_str);
            git_service::submodule_init_update(cwd, path)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "submodule.open" => {
            let path = request
                .paths
                .first()
                .ok_or_else(|| JobExecutionError::InvalidInput {
                    message: "submodule.open requires path in request.paths[0]".to_string(),
                })?;
            refresh_repo_and_status_for_path(&cwd.join(path), store)?;
            refresh_refs(&cwd.join(path), store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "diagnostics.repo_capabilities" => {
            let caps = git_service::repo_capabilities(cwd)?;
            let text = format!(
                "linked_worktree: {}\nhas_submodules: {}\nlfs_detected: {}",
                caps.is_linked_worktree, caps.has_submodules, caps.lfs_detected
            );
            store.update_diff(build_diff_state(
                DiffSource::Commit {
                    oid: "repo:capabilities".to_string(),
                },
                text,
                Vec::new(),
            ));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "stash.apply" => {
            let reference = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "stash.apply requires stash ref in request.paths[0]".to_string(),
                }
            })?;
            git_service::stash_apply(cwd, reference)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "stash.pop" => {
            let reference = request.paths.first().map(String::as_str);
            git_service::stash_pop(cwd, reference)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "stash.drop" => {
            let reference = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "stash.drop requires stash ref in request.paths[0]".to_string(),
                }
            })?;
            git_service::stash_drop(cwd, reference)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "history.select_commit" => {
            let oid = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "history.select_commit requires commit oid in request.paths[0]"
                        .to_string(),
                }
            })?;
            store.update_selected_commit(Some(oid.to_string()));
            let details = git_service::commit_details(cwd, oid)?;
            store.update_commit_details(CommitDetails {
                oid: details.oid,
                author: details.author,
                time: details.time,
                message: details.message,
            });
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "history.details" => {
            let oid = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "history.details requires commit oid in request.paths[0]".to_string(),
                }
            })?;
            let details = git_service::commit_details(cwd, oid)?;
            store.update_commit_details(CommitDetails {
                oid: details.oid,
                author: details.author,
                time: details.time,
                message: details.message,
            });
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "diff.worktree" => {
            let diff = git_service::diff_worktree_with_hunks(cwd, &request.paths, 256 * 1024)?;
            let source = DiffSource::Worktree {
                paths: request.paths.clone(),
            };
            let mapped_hunks = diff
                .hunks
                .into_iter()
                .map(|hunk| state_store::DiffHunk {
                    file_path: hunk.file_path,
                    hunk_index: hunk.hunk_index,
                    header: hunk.header,
                    lines: hunk.lines,
                })
                .collect();
            store.update_diff(build_diff_state(source, diff.text, mapped_hunks));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "diff.commit" => {
            let oid = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "diff.commit requires commit oid in request.paths[0]".to_string(),
                }
            })?;
            let diff = git_service::diff_commit(cwd, oid, 64 * 1024)?;
            let source = DiffSource::Commit {
                oid: oid.to_string(),
            };
            store.update_diff(build_diff_state(source, diff, Vec::new()));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "diff.index" => {
            let diff = git_service::diff_index_with_hunks(cwd, &request.paths, 256 * 1024)?;
            let source = DiffSource::Index {
                paths: request.paths.clone(),
            };
            let mapped_hunks = diff
                .hunks
                .into_iter()
                .map(|hunk| state_store::DiffHunk {
                    file_path: hunk.file_path,
                    hunk_index: hunk.hunk_index,
                    header: hunk.header,
                    lines: hunk.lines,
                })
                .collect();
            store.update_diff(build_diff_state(source, diff.text, mapped_hunks));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "compare.refs" => {
            let base_ref = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "compare.refs requires base ref in request.paths[0]".to_string(),
                }
            })?;
            let head_ref = request.paths.get(1).map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "compare.refs requires head ref in request.paths[1]".to_string(),
                }
            })?;
            let compare = git_service::compare_refs(cwd, base_ref, head_ref, 100)?;
            let compare_commits = compare
                .commits
                .iter()
                .map(|commit| state_store::CommitSummary {
                    oid: commit.oid.clone(),
                    author: commit.author.clone(),
                    time: commit.time.clone(),
                    summary: commit.summary.clone(),
                })
                .collect();
            store.update_compare_summary(
                compare.base_ref.clone(),
                compare.head_ref.clone(),
                compare.ahead,
                compare.behind,
                compare_commits,
            );
            let diff = git_service::diff_compare_with_hunks(cwd, base_ref, head_ref, 256 * 1024)?;
            let source = DiffSource::Compare {
                base: base_ref.to_string(),
                head: head_ref.to_string(),
            };
            let mapped_hunks = diff
                .hunks
                .into_iter()
                .map(|hunk| state_store::DiffHunk {
                    file_path: hunk.file_path,
                    hunk_index: hunk.hunk_index,
                    header: hunk.header,
                    lines: hunk.lines,
                })
                .collect();
            store.update_diff(build_diff_state(source, diff.text, mapped_hunks));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "merge.execute" => {
            ensure_no_conflicts(cwd, "merge.execute")?;
            let source_ref = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "merge.execute requires source ref in request.paths[0]".to_string(),
                }
            })?;
            let mode = parse_merge_mode(request.paths.get(1).map(String::as_str).unwrap_or("ff"))?;
            match git_service::merge_ref(cwd, source_ref, mode) {
                Ok(()) => {
                    refresh_after_advanced_op(cwd, store)?;
                    Ok(JobExecutionResult {
                        op: request.op.clone(),
                        success: true,
                        state_version: store.snapshot().version,
                    })
                }
                Err(err) => {
                    // Merge conflicts leave repo in merge state; refresh snapshot to expose partial state.
                    let _ = refresh_repo_and_status(cwd, store);
                    Err(JobExecutionError::from(err))
                }
            }
        }
        "merge.abort" => {
            git_service::merge_abort(cwd)?;
            refresh_after_advanced_op(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "cherry_pick.commit" => {
            ensure_no_conflicts(cwd, "cherry_pick.commit")?;
            let oid = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "cherry_pick.commit requires commit oid in request.paths[0]"
                        .to_string(),
                }
            })?;
            match git_service::cherry_pick_commit(cwd, oid) {
                Ok(()) => {
                    refresh_after_advanced_op(cwd, store)?;
                    Ok(JobExecutionResult {
                        op: request.op.clone(),
                        success: true,
                        state_version: store.snapshot().version,
                    })
                }
                Err(err) => {
                    let _ = refresh_repo_and_status(cwd, store);
                    Err(JobExecutionError::from(err))
                }
            }
        }
        "cherry_pick.abort" => {
            git_service::cherry_pick_abort(cwd)?;
            refresh_after_advanced_op(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "revert.commit" => {
            ensure_no_conflicts(cwd, "revert.commit")?;
            let oid = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "revert.commit requires commit oid in request.paths[0]".to_string(),
                }
            })?;
            git_service::revert_commit(cwd, oid)?;
            refresh_after_advanced_op(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "reset.refs" => {
            ensure_no_conflicts(cwd, "reset.refs")?;
            let mode_raw = request.paths.first().map(String::as_str).unwrap_or("mixed");
            let mode = parse_reset_mode(mode_raw)?;
            let target = request.paths.get(1).map(String::as_str).unwrap_or("HEAD");
            git_service::reset_ref(cwd, mode, target)?;
            refresh_after_advanced_op(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "rebase.plan.create" => {
            ensure_no_conflicts(cwd, "rebase.plan.create")?;
            let base_ref = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "rebase.plan.create requires base ref in request.paths[0]".to_string(),
                }
            })?;
            let plan = git_service::create_rebase_plan(cwd, base_ref)?;
            store.update_rebase_plan(map_rebase_plan(plan));
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "rebase.execute" => {
            ensure_no_conflicts(cwd, "rebase.execute")?;
            let plan = match store.snapshot().rebase.plan.as_ref() {
                Some(plan) => plan.clone(),
                None => {
                    return Err(JobExecutionError::InvalidInput {
                        message: "rebase.execute requires existing RebasePlan".to_string(),
                    });
                }
            };
            let autosquash = request
                .paths
                .first()
                .map(|v| v == "autosquash")
                .unwrap_or(false);
            match git_service::execute_rebase_plan(cwd, &map_rebase_plan_to_git(&plan), autosquash)
            {
                Ok(()) => {
                    sync_rebase_session_state(cwd, store)?;
                    if store.snapshot().rebase.session.is_none() {
                        refresh_after_advanced_op(cwd, store)?;
                        store.clear_rebase_plan();
                    }
                    Ok(JobExecutionResult {
                        op: request.op.clone(),
                        success: true,
                        state_version: store.snapshot().version,
                    })
                }
                Err(err) => {
                    let _ = sync_rebase_session_state(cwd, store);
                    let _ = refresh_repo_and_status(cwd, store);
                    Err(JobExecutionError::from(err))
                }
            }
        }
        "rebase.continue" => {
            git_service::rebase_continue(cwd)?;
            sync_rebase_session_state(cwd, store)?;
            if store.snapshot().rebase.session.is_none() {
                refresh_after_advanced_op(cwd, store)?;
                store.clear_rebase_plan();
            }
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "rebase.skip" => {
            git_service::rebase_skip(cwd)?;
            sync_rebase_session_state(cwd, store)?;
            if store.snapshot().rebase.session.is_none() {
                refresh_after_advanced_op(cwd, store)?;
                store.clear_rebase_plan();
            }
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "rebase.abort" => {
            git_service::rebase_abort(cwd)?;
            sync_rebase_session_state(cwd, store)?;
            store.clear_rebase_plan();
            refresh_after_advanced_op(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "conflict.list" => {
            let conflicts = git_service::list_conflicted_files(cwd)?;
            refresh_repo_and_status(cwd, store)?;
            store.update_selected_paths(conflicts.into_iter().map(|item| item.path).collect());
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "conflict.resolve.ours" => {
            let path = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "conflict.resolve.ours requires path in request.paths[0]".to_string(),
                }
            })?;
            git_service::conflict_apply_choice(cwd, path, git_service::ConflictChoice::Ours)?;
            refresh_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "conflict.resolve.theirs" => {
            let path = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "conflict.resolve.theirs requires path in request.paths[0]"
                        .to_string(),
                }
            })?;
            git_service::conflict_apply_choice(cwd, path, git_service::ConflictChoice::Theirs)?;
            refresh_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "conflict.mark_resolved" => {
            if request.paths.is_empty() {
                return Err(JobExecutionError::InvalidInput {
                    message: "conflict.mark_resolved requires at least one path".to_string(),
                });
            }
            git_service::mark_conflict_resolved(cwd, &request.paths)?;
            refresh_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "conflict.continue" => match git_service::conflict_continue(cwd) {
            Ok(()) => {
                sync_rebase_session_state(cwd, store)?;
                if store.snapshot().rebase.session.is_none() {
                    refresh_after_advanced_op(cwd, store)?;
                } else {
                    refresh_repo_and_status(cwd, store)?;
                }
                Ok(JobExecutionResult {
                    op: request.op.clone(),
                    success: true,
                    state_version: store.snapshot().version,
                })
            }
            Err(err) => {
                let _ = refresh_repo_and_status(cwd, store);
                Err(JobExecutionError::from(err))
            }
        },
        "conflict.abort" => {
            git_service::conflict_abort(cwd)?;
            sync_rebase_session_state(cwd, store)?;
            refresh_after_advanced_op(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "branch.checkout" => {
            ensure_no_conflicts(cwd, "branch.checkout")?;
            let name = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "branch.checkout requires name in request.paths[0]".to_string(),
                }
            })?;
            let clean = git_service::worktree_is_clean(cwd)?;
            if !clean {
                return Err(JobExecutionError::InvalidInput {
                    message: "Working tree has uncommitted changes.".to_string(),
                });
            }
            git_service::checkout_branch(cwd, name)?;
            refresh_repo_and_status(cwd, store)?;
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "branch.create" => {
            ensure_no_conflicts(cwd, "branch.create")?;
            let name = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "branch.create requires name in request.paths[0]".to_string(),
                }
            })?;
            git_service::create_branch(cwd, name)?;
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "branch.rename" => {
            ensure_no_conflicts(cwd, "branch.rename")?;
            let old = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "branch.rename requires old name in request.paths[0]".to_string(),
                }
            })?;
            let new = request.paths.get(1).map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "branch.rename requires new name in request.paths[1]".to_string(),
                }
            })?;
            git_service::rename_branch(cwd, old, new)?;
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "branch.delete" => {
            ensure_no_conflicts(cwd, "branch.delete")?;
            let name = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "branch.delete requires name in request.paths[0]".to_string(),
                }
            })?;
            let branches = git_service::list_local_branches(cwd)?;
            if branches
                .iter()
                .any(|branch| branch.name == name && branch.is_current)
            {
                return Err(JobExecutionError::InvalidInput {
                    message: "Cannot delete current branch.".to_string(),
                });
            }
            git_service::delete_branch(cwd, name)?;
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "tag.create" => {
            ensure_no_conflicts(cwd, "tag.create")?;
            let name = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "tag.create requires name in request.paths[0]".to_string(),
                }
            })?;
            git_service::create_tag(cwd, name)?;
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "tag.delete" => {
            ensure_no_conflicts(cwd, "tag.delete")?;
            let name = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "tag.delete requires name in request.paths[0]".to_string(),
                }
            })?;
            let _target = git_service::inspect_tag_target(cwd, name)?;
            git_service::delete_tag(cwd, name)?;
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "tag.checkout" => {
            ensure_no_conflicts(cwd, "tag.checkout")?;
            let name = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "tag.checkout requires name in request.paths[0]".to_string(),
                }
            })?;
            let clean = git_service::worktree_is_clean(cwd)?;
            if !clean {
                return Err(JobExecutionError::InvalidInput {
                    message: "Working tree has uncommitted changes.".to_string(),
                });
            }
            git_service::checkout_tag(cwd, name)?;
            refresh_repo_and_status(cwd, store)?;
            refresh_refs(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "file.discard" => {
            ensure_no_conflicts(cwd, "file.discard")?;
            if request.paths.is_empty() {
                return Err(JobExecutionError::InvalidInput {
                    message: "file.discard requires at least one file path".to_string(),
                });
            }
            git_service::discard_paths(cwd, &request.paths)?;
            refresh_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "commit.amend" => {
            ensure_no_conflicts(cwd, "commit.amend")?;
            let message = request.paths.first().map(String::as_str);
            git_service::commit_amend(cwd, message)?;
            refresh_repo_and_status(cwd, store)?;
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        _ => Err(JobExecutionError::UnsupportedOp {
            op: request.op.clone(),
        }),
    })();

    finish_journal_entry(store, journal_entry, &result, cwd, request.op.as_str());
    result
}

fn apply_repo_open(store: &mut StateStore, repo: &RepoOpenResult, status: &StatusSummary) {
    store.update_repo(RepoSnapshot {
        root: repo.root.clone(),
        head: repo.head.clone(),
        conflict_state: map_conflict_state(repo.conflict_state.as_ref()),
    });
    apply_status(store, status);
    store.update_selection(SelectionState::default());
    store.clear_history();
    store.clear_compare();
    store.update_diff(DiffState::default());
    store.set_active_view(Some("status.panel".to_string()));
}

fn map_conflict_state(state: Option<&git_service::ConflictState>) -> Option<ConflictState> {
    match state {
        Some(git_service::ConflictState::Merge) => Some(ConflictState::Merge),
        Some(git_service::ConflictState::Rebase) => Some(ConflictState::Rebase),
        Some(git_service::ConflictState::CherryPick) => Some(ConflictState::CherryPick),
        None => None,
    }
}

fn ensure_no_conflicts(cwd: &Path, op: &str) -> Result<(), JobExecutionError> {
    let conflict = git_service::detect_conflict_state(cwd)?;
    if let Some(state) = conflict {
        return Err(JobExecutionError::InvalidInput {
            message: format!(
                "{op} blocked while repository is in {} state.",
                conflict_state_label(&state)
            ),
        });
    }
    Ok(())
}

fn conflict_state_label(state: &git_service::ConflictState) -> &'static str {
    match state {
        git_service::ConflictState::Merge => "merge",
        git_service::ConflictState::Rebase => "rebase",
        git_service::ConflictState::CherryPick => "cherry-pick",
    }
}

fn apply_status(store: &mut StateStore, status: &StatusSummary) {
    store.update_status(StatusSnapshot {
        staged: status.staged.clone(),
        unstaged: status.unstaged.clone(),
        untracked: status.untracked.clone(),
    });
}

fn refresh_status(cwd: &Path, store: &mut StateStore) -> Result<(), JobExecutionError> {
    let status = git_service::status_refresh(cwd)?;
    apply_status(store, &status);
    Ok(())
}

fn refresh_repo_and_status(cwd: &Path, store: &mut StateStore) -> Result<(), JobExecutionError> {
    let repo = git_service::repo_open(cwd)?;
    let status = git_service::status_refresh(cwd)?;
    apply_repo_open(store, &repo, &status);
    Ok(())
}

fn refresh_repo_and_status_for_path(
    repo_path: &Path,
    store: &mut StateStore,
) -> Result<(), JobExecutionError> {
    let repo = git_service::repo_open(repo_path)?;
    let status = git_service::status_refresh(repo_path)?;
    apply_repo_open(store, &repo, &status);
    Ok(())
}

fn refresh_refs(cwd: &Path, store: &mut StateStore) -> Result<(), JobExecutionError> {
    let branches = git_service::list_local_branches(cwd)?;
    let mapped = branches
        .into_iter()
        .map(|branch| BranchInfo {
            name: branch.name,
            is_current: branch.is_current,
            upstream: branch.upstream,
        })
        .collect();
    store.update_branches(mapped);
    let tags = git_service::list_tags(cwd)?;
    let mapped_tags = tags
        .into_iter()
        .map(|name| state_store::TagInfo { name })
        .collect();
    store.update_tags(mapped_tags);
    Ok(())
}

fn refresh_after_advanced_op(cwd: &Path, store: &mut StateStore) -> Result<(), JobExecutionError> {
    refresh_repo_and_status(cwd, store)?;
    refresh_refs(cwd, store)?;
    store.clear_history();
    store.clear_compare();
    store.update_diff(DiffState::default());
    store.update_selection(SelectionState::default());
    Ok(())
}

fn refresh_loaded_diff(cwd: &Path, store: &mut StateStore) -> Result<(), JobExecutionError> {
    let source = store.snapshot().diff.source.clone();
    let Some(source) = source else {
        return Ok(());
    };

    match source {
        DiffSource::Worktree { paths } => {
            let diff = git_service::diff_worktree_with_hunks(cwd, &paths, 256 * 1024)?;
            let mapped_hunks = diff
                .hunks
                .into_iter()
                .map(|hunk| state_store::DiffHunk {
                    file_path: hunk.file_path,
                    hunk_index: hunk.hunk_index,
                    header: hunk.header,
                    lines: hunk.lines,
                })
                .collect();
            store.update_diff(build_diff_state(
                DiffSource::Worktree { paths },
                diff.text,
                mapped_hunks,
            ));
        }
        DiffSource::Index { paths } => {
            let diff = git_service::diff_index_with_hunks(cwd, &paths, 256 * 1024)?;
            let mapped_hunks = diff
                .hunks
                .into_iter()
                .map(|hunk| state_store::DiffHunk {
                    file_path: hunk.file_path,
                    hunk_index: hunk.hunk_index,
                    header: hunk.header,
                    lines: hunk.lines,
                })
                .collect();
            store.update_diff(build_diff_state(
                DiffSource::Index { paths },
                diff.text,
                mapped_hunks,
            ));
        }
        DiffSource::Commit { oid } => {
            let diff = git_service::diff_commit(cwd, &oid, 64 * 1024)?;
            store.update_diff(build_diff_state(
                DiffSource::Commit { oid },
                diff,
                Vec::new(),
            ));
        }
        DiffSource::Compare { base, head } => {
            let diff = git_service::diff_compare_with_hunks(cwd, &base, &head, 256 * 1024)?;
            let mapped_hunks = diff
                .hunks
                .into_iter()
                .map(|hunk| state_store::DiffHunk {
                    file_path: hunk.file_path,
                    hunk_index: hunk.hunk_index,
                    header: hunk.header,
                    lines: hunk.lines,
                })
                .collect();
            store.update_diff(build_diff_state(
                DiffSource::Compare { base, head },
                diff.text,
                mapped_hunks,
            ));
        }
    }
    Ok(())
}

fn parse_merge_mode(raw: &str) -> Result<git_service::MergeMode, JobExecutionError> {
    match raw {
        "ff" | "fast-forward" => Ok(git_service::MergeMode::FastForward),
        "no-ff" => Ok(git_service::MergeMode::NoFastForward),
        "squash" => Ok(git_service::MergeMode::Squash),
        _ => Err(JobExecutionError::InvalidInput {
            message: "merge.execute mode must be one of: ff, no-ff, squash".to_string(),
        }),
    }
}

fn parse_reset_mode(raw: &str) -> Result<git_service::ResetMode, JobExecutionError> {
    match raw {
        "soft" => Ok(git_service::ResetMode::Soft),
        "mixed" => Ok(git_service::ResetMode::Mixed),
        "hard" => Ok(git_service::ResetMode::Hard),
        _ => Err(JobExecutionError::InvalidInput {
            message: "reset.refs mode must be one of: soft, mixed, hard".to_string(),
        }),
    }
}

fn map_rebase_plan(plan: git_service::RebasePlan) -> RebasePlan {
    let rewrite_types = plan
        .entries
        .iter()
        .map(|entry| match entry.action {
            git_service::RebaseAction::Pick => "pick",
            git_service::RebaseAction::Reword => "reword",
            git_service::RebaseAction::Edit => "edit",
            git_service::RebaseAction::Squash => "squash",
            git_service::RebaseAction::Fixup => "fixup",
            git_service::RebaseAction::Drop => "drop",
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(str::to_string)
        .collect();

    RebasePlan {
        base_ref: plan.base_ref,
        base_oid: plan.base_oid,
        affected_commit_count: plan.entries.len(),
        rewrite_types,
        published_history_warning: plan.published_history_warning,
        autosquash_aware: plan.autosquash_aware,
        entries: plan
            .entries
            .into_iter()
            .map(|entry| RebasePlanEntry {
                oid: entry.oid,
                summary: entry.summary,
                warnings: entry.warnings,
                action: match entry.action {
                    git_service::RebaseAction::Pick => RebaseEntryAction::Pick,
                    git_service::RebaseAction::Reword => RebaseEntryAction::Reword,
                    git_service::RebaseAction::Edit => RebaseEntryAction::Edit,
                    git_service::RebaseAction::Squash => RebaseEntryAction::Squash,
                    git_service::RebaseAction::Fixup => RebaseEntryAction::Fixup,
                    git_service::RebaseAction::Drop => RebaseEntryAction::Drop,
                },
            })
            .collect(),
    }
}

fn map_rebase_plan_to_git(plan: &RebasePlan) -> git_service::RebasePlan {
    git_service::RebasePlan {
        base_ref: plan.base_ref.clone(),
        base_oid: plan.base_oid.clone(),
        autosquash_aware: plan.autosquash_aware,
        published_history_warning: plan.published_history_warning.clone(),
        entries: plan
            .entries
            .iter()
            .cloned()
            .map(|entry| git_service::RebasePlanEntry {
                oid: entry.oid,
                summary: entry.summary,
                warnings: entry.warnings,
                action: match entry.action {
                    RebaseEntryAction::Pick => git_service::RebaseAction::Pick,
                    RebaseEntryAction::Reword => git_service::RebaseAction::Reword,
                    RebaseEntryAction::Edit => git_service::RebaseAction::Edit,
                    RebaseEntryAction::Squash => git_service::RebaseAction::Squash,
                    RebaseEntryAction::Fixup => git_service::RebaseAction::Fixup,
                    RebaseEntryAction::Drop => git_service::RebaseAction::Drop,
                },
            })
            .collect(),
    }
}

fn sync_rebase_session_state(cwd: &Path, store: &mut StateStore) -> Result<(), JobExecutionError> {
    let hook = git_service::detect_rebase_session_hook(cwd)?;
    match hook {
        Some(hook) => {
            let repo_root = git_service::repo_open(cwd).ok().map(|repo| repo.root);
            store.update_rebase_session(RebaseSessionSnapshot {
                active: hook.active,
                repo_root,
                base_ref: store
                    .snapshot()
                    .rebase
                    .plan
                    .as_ref()
                    .map(|plan| plan.base_ref.clone()),
                current_step: hook.current_step,
                total_steps: hook.total_steps,
                blocking_conflict: matches!(git_service::detect_conflict_state(cwd), Ok(Some(_))),
            });
        }
        None => store.clear_rebase_session(),
    }
    Ok(())
}

fn parse_history_request(request: &JobRequest) -> Result<(usize, usize), JobExecutionError> {
    let offset = request
        .paths
        .first()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = request
        .paths
        .get(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);
    Ok((offset, limit))
}

struct HistoryLoadRequest<'a> {
    op: &'a str,
    offset: usize,
    limit: usize,
    append: bool,
    filter_author: Option<String>,
    filter_text: Option<String>,
    hash_prefix: Option<String>,
}

fn load_history_page(
    cwd: &Path,
    store: &mut StateStore,
    request: HistoryLoadRequest<'_>,
) -> Result<JobExecutionResult, JobExecutionError> {
    let HistoryLoadRequest {
        op,
        offset,
        limit,
        append,
        filter_author,
        filter_text,
        hash_prefix,
    } = request;
    store.set_history_loading(true);
    let commits = git_service::commit_log_page_filtered_with_hash_prefix(
        cwd,
        offset,
        limit,
        filter_author.as_deref(),
        filter_text.as_deref(),
        hash_prefix.as_deref(),
    )
    .map_err(|err| {
        store.set_history_error(format!("{err:?}"));
        JobExecutionError::from(err)
    })?;
    let commit_len = commits.len();
    let history_commits = commits
        .into_iter()
        .map(|commit| state_store::CommitSummary {
            oid: commit.oid,
            author: commit.author,
            time: commit.time,
            summary: commit.summary,
        })
        .collect();
    let next_cursor = if commit_len == limit {
        Some(HistoryCursor {
            offset: offset + commit_len,
            page_size: limit,
        })
    } else {
        None
    };
    store.update_history_page(
        history_commits,
        next_cursor,
        append,
        filter_author,
        filter_text,
    );
    Ok(JobExecutionResult {
        op: op.to_string(),
        success: true,
        state_version: store.snapshot().version,
    })
}

fn build_diff_state(
    source: DiffSource,
    text: String,
    hunks: Vec<state_store::DiffHunk>,
) -> DiffState {
    const CHUNK_SIZE: usize = 32 * 1024;
    let chunks = split_diff_chunks(&text, CHUNK_SIZE);
    let descriptor = DiffDescriptor {
        total_bytes: text.len(),
        chunk_size: CHUNK_SIZE,
        loaded_chunks: chunks.len(),
        truncated: text.contains("... diff truncated ..."),
    };

    DiffState {
        source: Some(source.clone()),
        descriptor: Some(descriptor),
        load_request: Some(DiffLoadRequest {
            source,
            chunk_size: CHUNK_SIZE,
            cursor: None,
        }),
        chunks,
        content: Some(text),
        hunks,
        loading: false,
        error: None,
    }
}

fn split_diff_chunks(text: &str, chunk_size: usize) -> Vec<DiffChunk> {
    if text.is_empty() {
        return Vec::new();
    }

    text.as_bytes()
        .chunks(chunk_size)
        .enumerate()
        .map(|(index, chunk)| DiffChunk {
            index,
            content: String::from_utf8_lossy(chunk).to_string(),
        })
        .collect()
}

fn start_journal_entry(
    store: &mut StateStore,
    request: &JobRequest,
    session_id: Option<u64>,
    pre_refs: Option<RefSnapshotSummary>,
) -> Option<u64> {
    if !is_journaled_op(&request.op) {
        return None;
    }
    let entry_id = store.append_journal_entry(request.job_id, request.op.clone(), now_millis());
    if let Some(session_id) = session_id {
        store.set_journal_session(
            entry_id,
            session_id,
            OperationSessionKind::AdvancedBranchOperation,
            OperationSessionState::Running,
        );
    }
    if let Some(pre_refs) = pre_refs {
        store.set_journal_pre_refs(entry_id, pre_refs);
    }
    Some(entry_id)
}

fn finish_journal_entry(
    store: &mut StateStore,
    entry_id: Option<u64>,
    result: &Result<JobExecutionResult, JobExecutionError>,
    cwd: &Path,
    op: &str,
) {
    let entry_id = match entry_id {
        Some(id) => id,
        None => return,
    };
    let status = if result.is_ok() {
        JournalStatus::Succeeded
    } else {
        JournalStatus::Failed
    };
    let session_state = if result.is_ok() {
        OperationSessionState::Succeeded
    } else {
        OperationSessionState::Failed
    };
    let error = result.as_ref().err().map(|err| format!("{err:?}"));
    store.finish_journal_entry(entry_id, status, now_millis(), error);
    if is_session_op(op) {
        store.set_journal_session_state(entry_id, session_state);
    }
    if is_ref_snapshot_op(op)
        && let Some(post_refs) = capture_ref_snapshot(cwd)
    {
        store.set_journal_post_refs(entry_id, post_refs);
    }
}

fn capture_ref_snapshot(cwd: &Path) -> Option<RefSnapshotSummary> {
    let repo = git_service::repo_open(cwd).ok()?;
    let branches = git_service::list_local_branches(cwd).ok()?;
    let tags = git_service::list_tags(cwd).ok()?;
    Some(RefSnapshotSummary {
        head: repo.head,
        branch_count: branches.len(),
        tag_count: tags.len(),
        conflict_state: map_conflict_state(repo.conflict_state.as_ref()),
    })
}

fn is_session_op(op: &str) -> bool {
    matches!(
        op,
        "merge.execute"
            | "merge.abort"
            | "cherry_pick.commit"
            | "cherry_pick.abort"
            | "revert.commit"
            | "reset.refs"
            | "rebase.execute"
            | "rebase.continue"
            | "rebase.skip"
            | "rebase.abort"
            | "conflict.continue"
            | "conflict.abort"
    )
}

fn is_ref_snapshot_op(op: &str) -> bool {
    is_session_op(op)
}

fn is_journaled_op(op: &str) -> bool {
    matches!(
        op,
        "index.stage_paths"
            | "index.unstage_paths"
            | "index.stage_hunk"
            | "index.unstage_hunk"
            | "file.discard_hunk"
            | "commit.create"
            | "commit.amend"
            | "branch.checkout"
            | "branch.create"
            | "branch.rename"
            | "branch.delete"
            | "merge.execute"
            | "merge.abort"
            | "cherry_pick.commit"
            | "cherry_pick.abort"
            | "revert.commit"
            | "reset.refs"
            | "rebase.plan.create"
            | "rebase.execute"
            | "rebase.continue"
            | "rebase.skip"
            | "rebase.abort"
            | "conflict.list"
            | "conflict.resolve.ours"
            | "conflict.resolve.theirs"
            | "conflict.mark_resolved"
            | "conflict.continue"
            | "conflict.abort"
            | "history.file"
            | "blame.file"
            | "stash.create"
            | "stash.list"
            | "stash.apply"
            | "stash.pop"
            | "stash.drop"
            | "worktree.list"
            | "worktree.create"
            | "worktree.remove"
            | "worktree.open"
            | "submodule.list"
            | "submodule.init_update"
            | "submodule.open"
            | "diagnostics.repo_capabilities"
            | "tag.create"
            | "tag.delete"
            | "tag.checkout"
            | "file.discard"
    )
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir() -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("branchforge-job-system-{nanos}-{seq}"))
    }

    #[test]
    fn maps_status_refresh() {
        let result = map_to_git_command("status.refresh");
        assert!(result.is_some());
    }

    #[test]
    fn queue_respects_lock_conflicts() {
        let mut queue = JobQueue::new();
        let read_id = queue.enqueue(JobRequest {
            op: "status.refresh".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        });
        let write_id = queue.enqueue(JobRequest {
            op: "index.stage_paths".to_string(),
            lock: JobLock::IndexWrite,
            paths: vec!["README.md".to_string()],
            job_id: None,
        });
        let read2_id = queue.enqueue(JobRequest {
            op: "status.refresh".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        });

        let first = queue.try_start_next();
        assert!(first.is_some());
        let second = queue.try_start_next();
        assert!(second.is_some());

        if let Some(job) = first {
            assert_eq!(job.id, read_id);
        }
        if let Some(job) = second {
            assert_eq!(job.id, read2_id);
        }

        assert_eq!(queue.running_count(), 2);
        let third = queue.try_start_next();
        assert!(third.is_none());

        let finished = queue.finish(read_id, true);
        assert!(finished.is_some());

        let still_blocked = queue.try_start_next();
        assert!(still_blocked.is_none());

        let finished2 = queue.finish(read2_id, true);
        assert!(finished2.is_some());

        let now_write = queue.try_start_next();
        assert!(now_write.is_some());
        if let Some(job) = now_write {
            assert_eq!(job.id, write_id);
            assert_eq!(job.state, JobState::Running);
        }
    }

    #[test]
    fn execute_repo_open_updates_store() {
        let repo_dir = unique_temp_dir();
        let create = std::fs::create_dir_all(&repo_dir);
        assert!(create.is_ok());

        let init = git_service::run_git(&repo_dir, &["init"]);
        assert!(init.is_ok());
        let write = std::fs::write(repo_dir.join("README.md"), "hello\n");
        assert!(write.is_ok());

        let mut store = StateStore::new();
        let req = JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        };
        let result = execute_job_op(&repo_dir, &req, &mut store);
        assert!(result.is_ok());

        let snapshot = store.snapshot();
        assert!(snapshot.repo.is_some());
        assert!(snapshot.status.untracked.iter().any(|p| p == "README.md"));
        assert!(snapshot.version >= 3);

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn repo_open_clears_selection_state() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

        let mut store = StateStore::new();
        store.update_selection(SelectionState {
            selected_paths: vec!["README.md".to_string()],
            selected_commit_oid: None,
            selected_branch: None,
        });
        assert_eq!(store.snapshot().selection.selected_paths.len(), 1);

        let req = JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        };
        assert!(execute_job_op(&repo_dir, &req, &mut store).is_ok());
        assert!(store.snapshot().selection.selected_paths.is_empty());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn execute_status_refresh_updates_store() {
        let repo_dir = unique_temp_dir();
        let create = std::fs::create_dir_all(&repo_dir);
        assert!(create.is_ok());

        let init = git_service::run_git(&repo_dir, &["init"]);
        assert!(init.is_ok());
        let write = std::fs::write(repo_dir.join("notes.txt"), "note\n");
        assert!(write.is_ok());

        let mut store = StateStore::new();
        let req = JobRequest {
            op: "status.refresh".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        };
        let result = execute_job_op(&repo_dir, &req, &mut store);
        assert!(result.is_ok());

        assert!(
            store
                .snapshot()
                .status
                .untracked
                .iter()
                .any(|p| p == "notes.txt")
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn execute_stage_and_unstage_paths_updates_status_groups() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());

        let file = "file.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "data\n").is_ok());

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "index.stage_paths".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec![file.clone()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(store.snapshot().status.staged.iter().any(|p| p == &file));

        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "index.unstage_paths".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec![file.clone()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(store.snapshot().status.untracked.iter().any(|p| p == &file));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn execute_stage_hunk_updates_status() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "hunk.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "line1\nline2\nline3\nline4\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        assert!(
            std::fs::write(repo_dir.join(&file), "line1-updated\nline2\nline3\nline4\n").is_ok()
        );

        let mut store = StateStore::new();
        let result = execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "index.stage_hunk".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec![file.clone(), "0".to_string()],
                job_id: None,
            },
            &mut store,
        );
        assert!(result.is_ok());
        assert!(store.snapshot().status.staged.iter().any(|p| p == &file));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn stage_refresh_matches_git_status() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());

        let file = "file.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "data\n").is_ok());

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "index.stage_paths".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec![file.clone()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );

        let status = git_service::status_refresh(&repo_dir).expect("status");
        assert_eq!(store.snapshot().status.staged, status.staged);
        assert_eq!(store.snapshot().status.unstaged, status.unstaged);
        assert_eq!(store.snapshot().status.untracked, status.untracked);

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn execute_commit_create_updates_repo_and_clears_staged() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "commit.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "data\n").is_ok());

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "index.stage_paths".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec![file.clone()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );

        let commit_result = execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "commit.create".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["Initial commit".to_string()],
                job_id: None,
            },
            &mut store,
        );
        assert!(commit_result.is_ok());

        let snapshot = store.snapshot();
        assert!(snapshot.repo.is_some());
        assert!(snapshot.status.staged.is_empty());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_refresh_matches_git_status() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "commit.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "data\n").is_ok());

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "index.stage_paths".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec![file.clone()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );

        let commit_result = execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "commit.create".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["Initial commit".to_string()],
                job_id: None,
            },
            &mut store,
        );
        assert!(commit_result.is_ok());

        let status = git_service::status_refresh(&repo_dir).expect("status");
        assert_eq!(store.snapshot().status.staged, status.staged);
        assert_eq!(store.snapshot().status.unstaged, status.unstaged);
        assert_eq!(store.snapshot().status.untracked, status.untracked);

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn history_page_appends_commits() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        assert!(std::fs::write(repo_dir.join("one.txt"), "one\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "commit one").is_ok());

        assert!(std::fs::write(repo_dir.join("two.txt"), "two\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["two.txt".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "commit two").is_ok());

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "history.page".to_string(),
                    lock: JobLock::Read,
                    paths: vec!["0".to_string(), "1".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert_eq!(store.snapshot().history.commits.len(), 1);

        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "history.page".to_string(),
                    lock: JobLock::Read,
                    paths: vec!["1".to_string(), "1".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert_eq!(store.snapshot().history.commits.len(), 2);

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn diff_commit_updates_store() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "diff.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "data\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(git_service::commit_create(&repo_dir, "commit diff").is_ok());

        let commits = git_service::commit_log_page(&repo_dir, 0, 1).expect("page");
        let oid = commits[0].oid.clone();

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "diff.commit".to_string(),
                    lock: JobLock::Read,
                    paths: vec![oid.clone()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(
            store
                .snapshot()
                .diff
                .content
                .as_deref()
                .unwrap_or("")
                .contains("commit")
        );
        assert!(store.snapshot().diff.descriptor.is_some());
        assert!(!store.snapshot().diff.chunks.is_empty());
        assert!(matches!(
            store
                .snapshot()
                .diff
                .load_request
                .as_ref()
                .map(|r| &r.source),
            Some(DiffSource::Commit { .. })
        ));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn branch_checkout_resets_history_and_diff_context() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "ctx.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "one\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "repo.open".to_string(),
                    lock: JobLock::Read,
                    paths: Vec::new(),
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "history.page".to_string(),
                    lock: JobLock::Read,
                    paths: vec!["0".to_string(), "20".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        let commits = store.snapshot().history.commits.clone();
        assert!(!commits.is_empty());
        let commit_oid = commits[0].oid.clone();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "diff.commit".to_string(),
                    lock: JobLock::Read,
                    paths: vec![commit_oid],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(store.snapshot().diff.source.is_some());

        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "branch.create".to_string(),
                    lock: JobLock::RefsWrite,
                    paths: vec!["feature/context".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "branch.checkout".to_string(),
                    lock: JobLock::RefsWrite,
                    paths: vec!["feature/context".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );

        assert!(store.snapshot().history.commits.is_empty());
        assert!(store.snapshot().diff.source.is_none());
        assert!(store.snapshot().diff.content.is_none());
        assert_eq!(
            store.snapshot().active_view.as_deref(),
            Some("status.panel")
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn delete_current_branch_is_blocked() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        let branches = git_service::list_local_branches(&repo_dir).expect("branches");
        let current = branches
            .iter()
            .find(|b| b.is_current)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| "main".to_string());

        let mut store = StateStore::new();
        let result = execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "branch.delete".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec![current],
                job_id: None,
            },
            &mut store,
        );
        assert!(matches!(
            result,
            Err(JobExecutionError::InvalidInput { message })
                if message == "Cannot delete current branch."
        ));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn checkout_blocked_when_dirty() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());
        assert!(git_service::create_branch(&repo_dir, "feature").is_ok());

        assert!(std::fs::write(repo_dir.join("README.md"), "dirty\n").is_ok());

        let mut store = StateStore::new();
        let result = execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "branch.checkout".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["feature".to_string()],
                job_id: None,
            },
            &mut store,
        );
        assert!(matches!(
            result,
            Err(JobExecutionError::InvalidInput { message })
                if message == "Working tree has uncommitted changes."
        ));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_create_fails_without_staged_changes() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

        let mut store = StateStore::new();
        let commit_result = execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "commit.create".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["Initial commit".to_string()],
                job_id: None,
            },
            &mut store,
        );
        assert!(commit_result.is_err());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn conflict_flow_merge_list_resolve_mark_and_continue() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("conflict.txt");
        assert!(std::fs::write(&file, "line\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        assert!(git_service::create_branch(&repo_dir, "feature").is_ok());
        assert!(git_service::checkout_branch(&repo_dir, "feature").is_ok());
        assert!(std::fs::write(&file, "feature\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "feature change").is_ok());

        assert!(
            git_service::checkout_branch(&repo_dir, "main").is_ok()
                || git_service::checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(std::fs::write(&file, "main\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "main change").is_ok());
        assert!(
            git_service::merge_ref(&repo_dir, "feature", git_service::MergeMode::NoFastForward)
                .is_err()
        );

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "conflict.list".to_string(),
                    lock: JobLock::Read,
                    paths: Vec::new(),
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(
            store
                .snapshot()
                .selection
                .selected_paths
                .iter()
                .any(|path| path == "conflict.txt")
        );

        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "conflict.resolve.ours".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec!["conflict.txt".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );

        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "conflict.mark_resolved".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec!["conflict.txt".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );

        let continued = execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "conflict.continue".to_string(),
                lock: JobLock::RefsWrite,
                paths: Vec::new(),
                job_id: None,
            },
            &mut store,
        );
        assert!(continued.is_ok());
        assert!(matches!(
            git_service::detect_conflict_state(&repo_dir),
            Ok(None)
        ));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn discard_hunk_updates_status_and_diff_state() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "discard.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "line1\nline2\nline3\nline4\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());
        assert!(
            std::fs::write(repo_dir.join(&file), "line1-updated\nline2\nline3\nline4\n").is_ok()
        );

        let mut store = StateStore::new();
        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "diff.worktree".to_string(),
                    lock: JobLock::Read,
                    paths: vec![file.clone()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(!store.snapshot().diff.hunks.is_empty());

        assert!(
            execute_job_op(
                &repo_dir,
                &JobRequest {
                    op: "file.discard_hunk".to_string(),
                    lock: JobLock::IndexWrite,
                    paths: vec![file.clone(), "0".to_string()],
                    job_id: None,
                },
                &mut store,
            )
            .is_ok()
        );

        assert!(store.snapshot().status.unstaged.is_empty());
        assert!(store.snapshot().diff.hunks.is_empty());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }
}
