use serde_json::Value;
use state_store::{
    CheckStatus, ProviderKind, ProviderRepository, PullRequestState, PullRequestSummary,
    ReviewState,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderApiConfig {
    pub github_api_base: Option<String>,
    pub gitlab_api_base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderApiError {
    pub message: String,
}

pub fn list_pull_requests(
    provider: &ProviderRepository,
    token: &str,
    config: &ProviderApiConfig,
) -> Result<Vec<PullRequestSummary>, ProviderApiError> {
    if token.trim().is_empty() {
        return Err(ProviderApiError {
            message: "provider API token is empty".to_string(),
        });
    }
    match provider.provider {
        ProviderKind::GitHub => list_github_pull_requests(provider, token, config),
        ProviderKind::GitLab => list_gitlab_merge_requests(provider, token, config),
        ProviderKind::Unknown => Err(ProviderApiError {
            message: "provider API listing requires GitHub.com or GitLab.com".to_string(),
        }),
    }
}

fn list_github_pull_requests(
    provider: &ProviderRepository,
    token: &str,
    config: &ProviderApiConfig,
) -> Result<Vec<PullRequestSummary>, ProviderApiError> {
    let base = config
        .github_api_base
        .as_deref()
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let url = format!(
        "{base}/repos/{}/{}/pulls?state=open&per_page=50",
        url_path_escape(&provider.owner),
        url_path_escape(&provider.repo)
    );
    let value = get_json(
        &url,
        &[
            ("user-agent", "BranchForge".to_string()),
            ("accept", "application/vnd.github+json".to_string()),
            ("authorization", format!("Bearer {token}")),
            ("x-github-api-version", "2022-11-28".to_string()),
        ],
        token,
    )?;
    let pulls = value.as_array().ok_or_else(|| ProviderApiError {
        message: "GitHub pull request response was not an array".to_string(),
    })?;
    let mut summaries = Vec::new();
    for pull in pulls {
        let mut summary = github_pull_summary(provider, pull)?;
        if let Some(sha) = value_str(pull, &["head", "sha"]) {
            summary.checks = match github_status_check(provider, sha, token, config) {
                Ok(checks) => checks,
                Err(err) => vec![state_store::CheckSummary {
                    name: "github-status".to_string(),
                    status: CheckStatus::Failure,
                    detail: Some(err.message),
                }],
            };
        }
        summaries.push(summary);
    }
    Ok(summaries)
}

fn github_status_check(
    provider: &ProviderRepository,
    sha: &str,
    token: &str,
    config: &ProviderApiConfig,
) -> Result<Vec<state_store::CheckSummary>, ProviderApiError> {
    let base = config
        .github_api_base
        .as_deref()
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let url = format!(
        "{base}/repos/{}/{}/commits/{}/status",
        url_path_escape(&provider.owner),
        url_path_escape(&provider.repo),
        url_path_escape(sha)
    );
    let value = get_json(
        &url,
        &[
            ("user-agent", "BranchForge".to_string()),
            ("accept", "application/vnd.github+json".to_string()),
            ("authorization", format!("Bearer {token}")),
            ("x-github-api-version", "2022-11-28".to_string()),
        ],
        token,
    )?;
    let state = value_str(&value, &["state"]).unwrap_or("pending");
    let count = value
        .get("statuses")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    Ok(vec![state_store::CheckSummary {
        name: "github-status".to_string(),
        status: map_provider_check_status(state),
        detail: Some(format!("combined status {state}; contexts={count}")),
    }])
}

fn github_pull_summary(
    provider: &ProviderRepository,
    value: &Value,
) -> Result<PullRequestSummary, ProviderApiError> {
    let number = value
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProviderApiError {
            message: "GitHub pull request missing number".to_string(),
        })?;
    let title = value_str(value, &["title"]).unwrap_or("").to_string();
    let author = value_str(value, &["user", "login"])
        .unwrap_or("unknown")
        .to_string();
    let source_branch = value_str(value, &["head", "ref"]).unwrap_or("").to_string();
    let target_branch = value_str(value, &["base", "ref"]).unwrap_or("").to_string();
    let draft = value.get("draft").and_then(Value::as_bool).unwrap_or(false);
    let state = if draft {
        PullRequestState::Draft
    } else {
        match value_str(value, &["state"]).unwrap_or("open") {
            "closed" if !value.get("merged_at").unwrap_or(&Value::Null).is_null() => {
                PullRequestState::Merged
            }
            "closed" => PullRequestState::Closed,
            _ => PullRequestState::Open,
        }
    };
    Ok(PullRequestSummary {
        provider: ProviderKind::GitHub,
        repo: format!("{}/{}", provider.owner, provider.repo),
        number,
        title,
        author,
        source_branch,
        target_branch,
        state,
        checks: Vec::new(),
        review_state: Some(ReviewState::ReviewRequired),
        web_url: value_str(value, &["html_url"]).map(str::to_string),
    })
}

fn list_gitlab_merge_requests(
    provider: &ProviderRepository,
    token: &str,
    config: &ProviderApiConfig,
) -> Result<Vec<PullRequestSummary>, ProviderApiError> {
    let base = config
        .gitlab_api_base
        .as_deref()
        .unwrap_or("https://gitlab.com/api/v4")
        .trim_end_matches('/');
    let project = url_query_escape(&format!("{}/{}", provider.owner, provider.repo));
    let url = format!("{base}/projects/{project}/merge_requests?state=opened&per_page=50");
    let value = get_json(
        &url,
        &[
            ("user-agent", "BranchForge".to_string()),
            ("accept", "application/json".to_string()),
            ("private-token", token.to_string()),
        ],
        token,
    )?;
    let merge_requests = value.as_array().ok_or_else(|| ProviderApiError {
        message: "GitLab merge request response was not an array".to_string(),
    })?;
    merge_requests
        .iter()
        .map(|mr| gitlab_merge_request_summary(provider, mr))
        .collect()
}

fn gitlab_merge_request_summary(
    provider: &ProviderRepository,
    value: &Value,
) -> Result<PullRequestSummary, ProviderApiError> {
    let number = value
        .get("iid")
        .or_else(|| value.get("id"))
        .and_then(Value::as_u64)
        .ok_or_else(|| ProviderApiError {
            message: "GitLab merge request missing iid".to_string(),
        })?;
    let draft = value
        .get("draft")
        .or_else(|| value.get("work_in_progress"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let state = if draft {
        PullRequestState::Draft
    } else {
        match value_str(value, &["state"]).unwrap_or("opened") {
            "merged" => PullRequestState::Merged,
            "closed" => PullRequestState::Closed,
            _ => PullRequestState::Open,
        }
    };
    let checks = value_str(value, &["head_pipeline", "status"])
        .map(|status| {
            vec![state_store::CheckSummary {
                name: "gitlab-pipeline".to_string(),
                status: map_provider_check_status(status),
                detail: Some(status.to_string()),
            }]
        })
        .unwrap_or_default();
    Ok(PullRequestSummary {
        provider: ProviderKind::GitLab,
        repo: format!("{}/{}", provider.owner, provider.repo),
        number,
        title: value_str(value, &["title"]).unwrap_or("").to_string(),
        author: value_str(value, &["author", "username"])
            .or_else(|| value_str(value, &["author", "name"]))
            .unwrap_or("unknown")
            .to_string(),
        source_branch: value_str(value, &["source_branch"])
            .unwrap_or("")
            .to_string(),
        target_branch: value_str(value, &["target_branch"])
            .unwrap_or("")
            .to_string(),
        state,
        checks,
        review_state: Some(ReviewState::ReviewRequired),
        web_url: value_str(value, &["web_url"]).map(str::to_string),
    })
}

fn get_json(url: &str, headers: &[(&str, String)], token: &str) -> Result<Value, ProviderApiError> {
    let mut request = ureq::get(url);
    for (name, value) in headers {
        request = request.header(*name, value.as_str());
    }
    let mut response = request.call().map_err(|err| ProviderApiError {
        message: redact_provider_error(format!("{err}"), token),
    })?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| ProviderApiError {
            message: format!("provider response read failed: {err}"),
        })?;
    serde_json::from_str(&body).map_err(|err| ProviderApiError {
        message: format!("provider response was not valid JSON: {err}"),
    })
}

fn value_str<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str()
}

fn map_provider_check_status(raw: &str) -> CheckStatus {
    match raw.to_ascii_lowercase().as_str() {
        "success" | "passed" => CheckStatus::Success,
        "failure" | "failed" | "error" => CheckStatus::Failure,
        "cancelled" | "canceled" | "skipped" => CheckStatus::Cancelled,
        _ => CheckStatus::Pending,
    }
}

fn redact_provider_error(raw: String, token: &str) -> String {
    if token.is_empty() {
        raw
    } else {
        raw.replace(token, "<redacted>")
    }
}

fn url_path_escape(value: &str) -> String {
    value
        .split('/')
        .map(url_query_escape)
        .collect::<Vec<_>>()
        .join("/")
}

fn url_query_escape(value: &str) -> String {
    let mut escaped = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            escaped.push(byte as char);
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(kind: ProviderKind) -> ProviderRepository {
        ProviderRepository {
            provider: kind,
            host: "example.com".to_string(),
            owner: "branchforge".to_string(),
            repo: "app".to_string(),
            web_url: "https://example.com/branchforge/app".to_string(),
        }
    }

    #[test]
    fn github_pull_parser_maps_summary_fields() {
        let raw = serde_json::json!({
            "number": 42,
            "title": "Add API",
            "state": "open",
            "draft": false,
            "html_url": "https://github.com/branchforge/app/pull/42",
            "user": {"login": "octo"},
            "head": {"ref": "feature/api", "sha": "abc"},
            "base": {"ref": "main"}
        });
        let summary =
            github_pull_summary(&provider(ProviderKind::GitHub), &raw).expect("github summary");
        assert_eq!(summary.number, 42);
        assert_eq!(summary.source_branch, "feature/api");
        assert_eq!(summary.target_branch, "main");
        assert_eq!(summary.state, PullRequestState::Open);
    }

    #[test]
    fn gitlab_merge_request_parser_maps_pipeline() {
        let raw = serde_json::json!({
            "iid": 7,
            "title": "MR API",
            "state": "opened",
            "web_url": "https://gitlab.com/branchforge/app/-/merge_requests/7",
            "author": {"username": "dev"},
            "source_branch": "feature/mr",
            "target_branch": "main",
            "head_pipeline": {"status": "failed"}
        });
        let summary = gitlab_merge_request_summary(&provider(ProviderKind::GitLab), &raw)
            .expect("gitlab summary");
        assert_eq!(summary.number, 7);
        assert_eq!(summary.checks[0].status, CheckStatus::Failure);
        assert_eq!(summary.provider, ProviderKind::GitLab);
    }
}
