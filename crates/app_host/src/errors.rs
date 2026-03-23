use git_service::GitServiceError;
use job_system::JobExecutionError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFacingError {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
}

impl UserFacingError {
    pub fn new(title: &str, message: &str, detail: Option<String>) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            detail,
        }
    }
}

pub fn translate_job_error(error: &JobExecutionError) -> UserFacingError {
    match error {
        JobExecutionError::InvalidInput { message } => {
            UserFacingError::new("Invalid input", message, None)
        }
        JobExecutionError::UnsupportedOp { op } => UserFacingError::new(
            "Unsupported operation",
            &format!("Operation `{op}` is not supported yet."),
            None,
        ),
        JobExecutionError::Git(git_error) => translate_git_error(git_error),
    }
}

fn translate_git_error(error: &GitServiceError) -> UserFacingError {
    match error {
        GitServiceError::ProcessLaunch(reason) => UserFacingError::new(
            "Git unavailable",
            "Failed to launch git. Ensure git is installed and on PATH.",
            Some(reason.clone()),
        ),
        GitServiceError::GitError { stderr, .. } => {
            let lowered = stderr.to_lowercase();
            if lowered.contains("not a git repository") {
                return UserFacingError::new(
                    "Not a Git repository",
                    "Select a folder that contains a Git repository.",
                    Some(stderr.clone()),
                );
            }
            if lowered.contains("nothing to commit") {
                return UserFacingError::new(
                    "Nothing to commit",
                    "Stage changes before committing.",
                    Some(stderr.clone()),
                );
            }
            if lowered.contains("pathspec") {
                return UserFacingError::new(
                    "File not found",
                    "One or more paths could not be staged.",
                    Some(stderr.clone()),
                );
            }

            UserFacingError::new(
                "Git error",
                "Git command failed. Check details for more information.",
                Some(stderr.clone()),
            )
        }
        GitServiceError::Utf8Decode => {
            UserFacingError::new("Git output error", "Failed to decode git output.", None)
        }
        GitServiceError::ParseError(message) => UserFacingError::new(
            "Git output error",
            "Failed to parse git output.",
            Some(message.clone()),
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
