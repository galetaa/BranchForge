use std::collections::{HashMap, VecDeque};
use std::path::Path;

use git_service::{GitCommand, GitServiceError, RepoOpenResult, StatusSummary};
use plugin_api::RepoSnapshot;
use state_store::{SelectionState, StateStore, StatusSnapshot};

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
    match request.op.as_str() {
        "repo.open" => {
            let repo = git_service::repo_open(cwd)?;
            let status = git_service::status_refresh(cwd)?;
            apply_repo_open(store, &repo, &status);
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "status.refresh" => {
            let status = git_service::status_refresh(cwd)?;
            apply_status(store, &status);
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "index.stage_paths" => {
            git_service::stage_paths(cwd, &request.paths)?;
            let status = git_service::status_refresh(cwd)?;
            apply_status(store, &status);
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "index.unstage_paths" => {
            git_service::unstage_paths(cwd, &request.paths)?;
            let status = git_service::status_refresh(cwd)?;
            apply_status(store, &status);
            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        "commit.create" => {
            let message = request.paths.first().map(String::as_str).ok_or_else(|| {
                JobExecutionError::InvalidInput {
                    message: "commit.create requires message in request.paths[0]".to_string(),
                }
            })?;

            git_service::commit_create(cwd, message)?;
            let repo = git_service::repo_open(cwd)?;
            let status = git_service::status_refresh(cwd)?;
            apply_repo_open(store, &repo, &status);

            Ok(JobExecutionResult {
                op: request.op.clone(),
                success: true,
                state_version: store.snapshot().version,
            })
        }
        _ => Err(JobExecutionError::UnsupportedOp {
            op: request.op.clone(),
        }),
    }
}

fn apply_repo_open(store: &mut StateStore, repo: &RepoOpenResult, status: &StatusSummary) {
    store.update_repo(RepoSnapshot {
        root: repo.root.clone(),
        head: repo.head.clone(),
    });
    apply_status(store, status);
    store.update_selection(SelectionState::default());
}

fn apply_status(store: &mut StateStore, status: &StatusSummary) {
    store.update_status(StatusSnapshot {
        staged: status.staged.clone(),
        unstaged: status.unstaged.clone(),
        untracked: status.untracked.clone(),
    });
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
        });
        let write_id = queue.enqueue(JobRequest {
            op: "index.stage_paths".to_string(),
            lock: JobLock::IndexWrite,
            paths: vec!["README.md".to_string()],
        });
        let read2_id = queue.enqueue(JobRequest {
            op: "status.refresh".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
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
                },
                &mut store,
            )
            .is_ok()
        );
        assert!(store.snapshot().status.untracked.iter().any(|p| p == &file));

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
            },
            &mut store,
        );
        assert!(commit_result.is_ok());

        let snapshot = store.snapshot();
        assert!(snapshot.repo.is_some());
        assert!(snapshot.status.staged.is_empty());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }
}
