#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    pub action: String,
}

pub fn validate_action(request: &ActionRequest) -> bool {
    !request.action.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_action() {
        let req = ActionRequest {
            action: String::new(),
        };
        assert!(!validate_action(&req));
    }
}
