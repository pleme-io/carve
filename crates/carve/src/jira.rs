//! JIRA REST client — minimal surface, only what carve needs.
//!
//! Auth: reads `ATLASSIAN_BASE_URL`, `ATLASSIAN_EMAIL`, and `ATLASSIAN_API_TOKEN`
//! from env. Operators who use the Atlassian MCP server set those already.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Carve's view of a JIRA sub-task — enough to fan an epic into stack
/// nodes. The full Jira issue payload is much larger; we keep only what
/// the plan needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub key: String,
    pub summary: String,
    pub status: String,
}

pub struct Client {
    base: String,
    email: String,
    token: String,
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn from_env() -> Result<Self> {
        let base = std::env::var("ATLASSIAN_BASE_URL")
            .context("ATLASSIAN_BASE_URL not set (e.g. https://akeyless.atlassian.net)")?;
        let email = std::env::var("ATLASSIAN_EMAIL").context("ATLASSIAN_EMAIL not set")?;
        let token =
            std::env::var("ATLASSIAN_API_TOKEN").context("ATLASSIAN_API_TOKEN not set")?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            email,
            token,
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// Fetch the direct children (sub-tasks) of an epic / parent issue.
    pub fn epic_children(&self, epic_key: &str) -> Result<Vec<SubTask>> {
        let jql = format!("parent = {epic_key} ORDER BY key ASC");
        let url = format!("{}/rest/api/2/search", self.base);
        #[derive(Deserialize)]
        struct SearchResp {
            issues: Vec<RawIssue>,
        }
        #[derive(Deserialize)]
        struct RawIssue {
            key: String,
            fields: RawFields,
        }
        #[derive(Deserialize)]
        struct RawFields {
            summary: String,
            status: RawStatus,
        }
        #[derive(Deserialize)]
        struct RawStatus {
            name: String,
        }
        let resp: SearchResp = self
            .http
            .get(&url)
            .basic_auth(&self.email, Some(&self.token))
            .query(&[("jql", jql.as_str()), ("fields", "summary,status")])
            .send()
            .context("GET /rest/api/2/search")?
            .error_for_status()?
            .json()?;
        Ok(resp
            .issues
            .into_iter()
            .map(|i| SubTask {
                key: i.key,
                summary: i.fields.summary,
                status: i.fields.status.name,
            })
            .collect())
    }
}
