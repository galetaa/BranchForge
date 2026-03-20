#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl GitCommand {
    pub fn status_porcelain() -> Self {
        Self {
            program: "git".to_string(),
            args: vec![
                "status".to_string(),
                "--porcelain=v2".to_string(),
                "-z".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_status_command() {
        let cmd = GitCommand::status_porcelain();
        assert_eq!(cmd.program, "git");
        assert_eq!(cmd.args.len(), 3);
    }
}
