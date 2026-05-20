//! JIRA REST client — minimal surface, only what carve needs.
//!
//! Auth: reads `ATLASSIAN_BASE_URL`, `ATLASSIAN_EMAIL`, and `ATLASSIAN_API_TOKEN`
//! from env. Operators who use the Atlassian MCP server set those already.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Carve's view of a JIRA sub-task — enough to fan an epic into stack
/// nodes. The full Jira issue payload is much larger; we keep only what
/// the plan needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub key: String,
    pub summary: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transition {
    pub id: String,
    pub name: String,
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

    /// Set a single custom field (typically story points) to a numeric
    /// value. Field id and value are policy-driven by [`carve_types::JiraConfig`].
    pub fn set_number_field(&self, issue_key: &str, field_id: &str, value: f32) -> Result<()> {
        let url = format!("{}/rest/api/2/issue/{}", self.base, issue_key);
        let body = json!({ "fields": { field_id: value } });
        let resp = self
            .http
            .put(&url)
            .basic_auth(&self.email, Some(&self.token))
            .json(&body)
            .send()
            .with_context(|| format!("PUT {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            anyhow::bail!("set_number_field({issue_key}, {field_id}={value}) failed: {status} — {text}");
        }
        Ok(())
    }

    /// List the transitions available from the issue's current state.
    pub fn transitions(&self, issue_key: &str) -> Result<Vec<Transition>> {
        let url = format!("{}/rest/api/2/issue/{}/transitions", self.base, issue_key);
        #[derive(Deserialize)]
        struct R {
            transitions: Vec<Transition>,
        }
        let r: R = self
            .http
            .get(&url)
            .basic_auth(&self.email, Some(&self.token))
            .send()?
            .error_for_status()?
            .json()?;
        Ok(r.transitions)
    }

    /// Apply a transition by id.
    pub fn transition(&self, issue_key: &str, transition_id: &str) -> Result<()> {
        let url = format!("{}/rest/api/2/issue/{}/transitions", self.base, issue_key);
        let body = json!({ "transition": { "id": transition_id } });
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.email, Some(&self.token))
            .json(&body)
            .send()?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            anyhow::bail!("transition({issue_key} → id={transition_id}) failed: {status} — {text}");
        }
        Ok(())
    }

    /// Post a plain-text comment to a JIRA issue. The Cloud REST API
    /// requires Atlassian Document Format (ADF) for the body, so we wrap
    /// the supplied text in a single paragraph node.
    #[allow(dead_code)]
    pub fn add_comment(&self, issue_key: &str, text: &str) -> Result<()> {
        let url = format!("{}/rest/api/3/issue/{}/comment", self.base, issue_key);
        let body = json!({
            "body": {
                "version": 1,
                "type": "doc",
                "content": [{
                    "type": "paragraph",
                    "content": [{
                        "type": "text",
                        "text": text,
                    }]
                }]
            }
        });
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.email, Some(&self.token))
            .json(&body)
            .send()?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            anyhow::bail!("add_comment({issue_key}) failed: {status} — {text}");
        }
        Ok(())
    }
}
