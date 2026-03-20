use git_service::GitCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

pub fn map_to_git_command(op: &str) -> Option<GitCommand> {
    match op {
        "status.refresh" => Some(GitCommand::status_porcelain()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_refresh() {
        let result = map_to_git_command("status.refresh");
        assert!(result.is_some());
    }
}
