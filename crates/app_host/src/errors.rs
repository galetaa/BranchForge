use git_service::GitServiceError;
use job_system::JobExecutionError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
    Validation,
    Repository,
    Conflicts,
    Refs,
    Git,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFacingError {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub category: ErrorCategory,
    pub correlation_id: String,
}

impl UserFacingError {
    pub fn new(title: &str, message: &str, detail: Option<String>) -> Self {
        Self::with_category(title, message, detail, ErrorCategory::Git)
    }

    pub fn with_category(
        title: &str,
        message: &str,
        detail: Option<String>,
        category: ErrorCategory,
    ) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            detail,
            category,
            correlation_id: next_correlation_id(),
        }
    }
}

fn next_correlation_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("bf-{ts}-{seq}")
}

pub fn translate_job_error(error: &JobExecutionError) -> UserFacingError {
    match error {
        JobExecutionError::InvalidInput { message } => {
            UserFacingError::with_category("Invalid input", message, None, ErrorCategory::Validation)
        }
        JobExecutionError::UnsupportedOp { op } => UserFacingError::with_category(
            "Unsupported operation",
            &format!("Operation `{op}` is not supported yet."),
            None,
            ErrorCategory::System,
        ),
        JobExecutionError::Git(git_error) => translate_git_error(git_error),
    }
}

fn translate_git_error(error: &GitServiceError) -> UserFacingError {
    match error {
        GitServiceError::ProcessLaunch(reason) => UserFacingError::with_category(
            "Git unavailable",
            "Failed to launch git. Ensure git is installed and on PATH.",
            Some(reason.clone()),
            ErrorCategory::System,
        ),
        GitServiceError::GitError { stderr, .. } => {
            let lowered = stderr.to_lowercase();
            if lowered.contains("not a git repository") {
                return UserFacingError::with_category(
                    "Not a Git repository",
                    "Select a folder that contains a Git repository.",
                    Some(stderr.clone()),
                    ErrorCategory::Repository,
                );
            }
            if lowered.contains("nothing to commit") {
                return UserFacingError::with_category(
                    "Nothing to commit",
                    "Stage changes before committing.",
                    Some(stderr.clone()),
                    ErrorCategory::Validation,
                );
            }
            if lowered.contains("you have nothing to amend") || lowered.contains("nothing to amend")
            {
                return UserFacingError::with_category(
                    "Nothing to amend",
                    "Create a commit before amending.",
                    Some(stderr.clone()),
                    ErrorCategory::Validation,
                );
            }
            if lowered.contains("would be overwritten by checkout")
                || lowered.contains("please commit your changes")
            {
                return UserFacingError::with_category(
                    "Working tree not clean",
                    "Commit or stash changes before checkout.",
                    Some(stderr.clone()),
                    ErrorCategory::Validation,
                );
            }
            if lowered.contains("not fully merged") {
                return UserFacingError::with_category(
                    "Branch not fully merged",
                    "Merge the branch or use a force delete.",
                    Some(stderr.clone()),
                    ErrorCategory::Refs,
                );
            }
            if lowered.contains("cannot delete branch") && lowered.contains("checked out") {
                return UserFacingError::with_category(
                    "Cannot delete current branch",
                    "Checkout another branch before deleting.",
                    Some(stderr.clone()),
                    ErrorCategory::Refs,
                );
            }
            if lowered.contains("already exists") {
                return UserFacingError::with_category(
                    "Name already exists",
                    "Use a different name or delete the existing ref.",
                    Some(stderr.clone()),
                    ErrorCategory::Refs,
                );
            }
            if lowered.contains("unknown revision")
                || lowered.contains("not a commit")
                || lowered.contains("not a valid object name")
            {
                return UserFacingError::with_category(
                    "Reference not found",
                    "Check the branch or tag name and try again.",
                    Some(stderr.clone()),
                    ErrorCategory::Refs,
                );
            }
            if lowered.contains("pathspec") {
                return UserFacingError::with_category(
                    "Not found",
                    "Check the reference or file path and try again.",
                    Some(stderr.clone()),
                    ErrorCategory::Repository,
                );
            }
            if lowered.contains("unmerged files") || lowered.contains("you have unmerged paths") {
                return UserFacingError::with_category(
                    "Unresolved conflicts",
                    "Resolve conflicts before continuing.",
                    Some(stderr.clone()),
                    ErrorCategory::Conflicts,
                );
            }

            UserFacingError::with_category(
                "Git error",
                "Git command failed. Check details for more information.",
                Some(stderr.clone()),
                ErrorCategory::Git,
            )
        }
        GitServiceError::Utf8Decode => UserFacingError::with_category(
            "Git output error",
            "Failed to decode git output.",
            None,
            ErrorCategory::System,
        ),
        GitServiceError::ParseError(message) => UserFacingError::with_category(
            "Git output error",
            "Failed to parse git output.",
            Some(message.clone()),
            ErrorCategory::System,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_not_a_repo_error_with_detail() {
        let err = JobExecutionError::Git(GitServiceError::GitError {
            exit_code: 128,
            stderr: "fatal: not a git repository (or any of the parent directories)".to_string(),
        });

        let translated = translate_job_error(&err);
        assert_eq!(translated.title, "Not a Git repository");
        assert_eq!(
            translated.message,
            "Select a folder that contains a Git repository."
        );
        assert!(translated.detail.is_some());
    }

    #[test]
    fn translates_nothing_to_commit() {
        let err = JobExecutionError::Git(GitServiceError::GitError {
            exit_code: 1,
            stderr: "nothing to commit, working tree clean".to_string(),
        });

        let translated = translate_job_error(&err);
        assert_eq!(translated.title, "Nothing to commit");
        assert_eq!(translated.message, "Stage changes before committing.");
    }
}
