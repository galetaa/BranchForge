use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcEnvelope {
    pub id: String,
    pub method: String,
    pub payload: serde_json::Value,
}

impl RpcEnvelope {
    pub fn new(
        id: impl Into<String>,
        method: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            method: method.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSnapshot {
    pub root: String,
    pub head: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_serializes() {
        let env = RpcEnvelope::new("1", "plugin.hello", serde_json::json!({"name": "status"}));
        let as_text = serde_json::to_string(&env);
        assert!(as_text.is_ok());
    }
}
