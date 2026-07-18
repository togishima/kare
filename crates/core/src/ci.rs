//! Best-effort detection of git ref and CI job URL from CI environment
//! variables.

/// Detected CI context, if any.
pub struct CiInfo {
    pub git_ref: Option<String>,
    pub job_url: Option<String>,
}

/// Detects CI info from `env`, a lookup function (injected for testability).
/// GitLab CI is preferred over GitHub Actions when both are present.
pub fn detect(env: &dyn Fn(&str) -> Option<String>) -> CiInfo {
    if let Some(git_ref) = env("CI_COMMIT_SHA") {
        return CiInfo {
            git_ref: Some(git_ref),
            job_url: env("CI_JOB_URL"),
        };
    }

    if let Some(git_ref) = env("GITHUB_SHA") {
        let job_url = match (
            env("GITHUB_SERVER_URL"),
            env("GITHUB_REPOSITORY"),
            env("GITHUB_RUN_ID"),
        ) {
            (Some(server), Some(repo), Some(run_id)) => {
                Some(format!("{server}/{repo}/actions/runs/{run_id}"))
            }
            _ => None,
        };
        return CiInfo {
            git_ref: Some(git_ref),
            job_url,
        };
    }

    CiInfo {
        git_ref: None,
        job_url: None,
    }
}
