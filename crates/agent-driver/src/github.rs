//! The GitHub REST client: find the installation, mint a narrowed token,
//! and the read operations. Tokens and JWTs only ever leave this module as
//! redacted types.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ureq::Agent;
use ureq::config::Config as UreqConfig;

use crate::error::Error;
use crate::jwt::Jwt;
use crate::repo::RepoName;

const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const USER_AGENT: &str = concat!("agent-driver/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Read,
    Write,
}

/// The permissions a token asks for. Absent means not granted, so each
/// operation asks for exactly what it needs.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Permissions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<Access>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issues: Option<Access>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_requests: Option<Access>,
}

/// Body of `POST /app/installations/{id}/access_tokens`: one repo, the
/// permissions named, nothing else.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TokenRequest {
    repositories: Vec<String>,
    permissions: Permissions,
}

impl TokenRequest {
    pub fn narrowed(repo: &RepoName, permissions: Permissions) -> Self {
        Self {
            repositories: vec![repo.name().to_owned()],
            permissions,
        }
    }
}

/// An installation token. Debug redacts it and there is no Display.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Token(String);

impl Token {
    fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

#[derive(Deserialize)]
struct Installation {
    id: u64,
}

/// A minted token and when GitHub will stop honouring it (RFC 3339).
#[derive(Deserialize)]
pub struct Minted {
    pub token: Token,
    pub expires_at: String,
}

/// An issue as the issues API returns it, cut to what the driver needs.
/// The API returns pull requests too; `pull_request` is present only on
/// those.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub state: State,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub labels: Vec<IssueLabel>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
}

impl Issue {
    pub fn is_pull_request(&self) -> bool {
        self.pull_request.is_some()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Open,
    Closed,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Open => "open",
            Self::Closed => "closed",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IssueLabel {
    pub name: String,
}

pub struct Client {
    agent: Agent,
}

impl Client {
    pub fn new() -> Self {
        let config = UreqConfig::builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Self {
            agent: Agent::new_with_config(config),
        }
    }

    fn get<T: serde::de::DeserializeOwned>(
        &self,
        what: &'static str,
        path: &str,
        authorization: &str,
    ) -> Result<T, Error> {
        let response = self
            .agent
            .get(format!("{API}{path}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("Authorization", authorization)
            .call()
            .map_err(|source| http_error(what, source))?;
        response
            .into_body()
            .read_json()
            .map_err(|source| http_error(what, source))
    }

    fn post<T: serde::de::DeserializeOwned>(
        &self,
        what: &'static str,
        path: &str,
        authorization: &str,
        body: &impl Serialize,
    ) -> Result<T, Error> {
        let response = self
            .agent
            .post(format!("{API}{path}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("Authorization", authorization)
            .send_json(body)
            .map_err(|source| http_error(what, source))?;
        response
            .into_body()
            .read_json()
            .map_err(|source| http_error(what, source))
    }

    fn patch<T: serde::de::DeserializeOwned>(
        &self,
        what: &'static str,
        path: &str,
        authorization: &str,
        body: &impl Serialize,
    ) -> Result<T, Error> {
        let response = self
            .agent
            .patch(format!("{API}{path}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("Authorization", authorization)
            .send_json(body)
            .map_err(|source| http_error(what, source))?;
        response
            .into_body()
            .read_json()
            .map_err(|source| http_error(what, source))
    }

    /// The App's installation on `repo`, or a refusal if there is none.
    pub fn installation_id(&self, jwt: &Jwt, repo: &RepoName) -> Result<u64, Error> {
        let path = format!("/repos/{}/{}/installation", repo.owner(), repo.name());
        let installation: Installation = self
            .get("installation lookup", &path, &jwt.bearer())
            .map_err(|e| match e {
                Error::Status { status: 404, .. } => Error::NotInstalled(repo.to_string()),
                other => other,
            })?;
        Ok(installation.id)
    }

    /// Mints a token for one installation, narrowed by `request`.
    pub fn mint(
        &self,
        jwt: &Jwt,
        installation: u64,
        request: &TokenRequest,
    ) -> Result<Minted, Error> {
        let path = format!("/app/installations/{installation}/access_tokens");
        self.post("token mint", &path, &jwt.bearer(), request)
    }

    pub fn issue(&self, token: &Token, repo: &RepoName, number: u64) -> Result<Issue, Error> {
        let path = format!("/repos/{}/{}/issues/{number}", repo.owner(), repo.name());
        self.get("issue view", &path, &token.bearer())
    }

    /// One page of issues, newest first. The API mixes in pull requests;
    /// the caller drops them.
    pub fn issues(
        &self,
        token: &Token,
        repo: &RepoName,
        query: &ListQuery,
    ) -> Result<Vec<Issue>, Error> {
        let path = format!(
            "/repos/{}/{}/issues?{}",
            repo.owner(),
            repo.name(),
            query.encode()
        );
        self.get("issue list", &path, &token.bearer())
    }

    pub fn comment(
        &self,
        token: &Token,
        repo: &RepoName,
        number: u64,
        body: &CommentBody,
    ) -> Result<Comment, Error> {
        let path = format!(
            "/repos/{}/{}/issues/{number}/comments",
            repo.owner(),
            repo.name()
        );
        self.post("issue comment", &path, &token.bearer(), body)
    }

    pub fn edit(
        &self,
        token: &Token,
        repo: &RepoName,
        number: u64,
        body: &EditBody,
    ) -> Result<Issue, Error> {
        let path = format!("/repos/{}/{}/issues/{number}", repo.owner(), repo.name());
        self.patch("issue edit", &path, &token.bearer(), body)
    }
}

/// Query of `GET /repos/{owner}/{repo}/issues`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQuery {
    pub state: &'static str,
    pub label: Option<String>,
    pub per_page: u8,
}

impl ListQuery {
    /// The query string, every value percent-encoded.
    pub fn encode(&self) -> String {
        let mut query = format!("state={}&per_page={}", encode(self.state), self.per_page);
        if let Some(label) = &self.label {
            query.push_str("&labels=");
            query.push_str(&encode(label));
        }
        query
    }
}

/// Percent-encodes everything but RFC 3986's unreserved characters.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Body of `POST .../issues/{n}/comments`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CommentBody {
    pub body: String,
}

/// Body of `PATCH .../issues/{n}`. Only `body`: the same endpoint closes,
/// relabels and retitles, and none of that may ride along.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct EditBody {
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct Comment {
    pub html_url: String,
}

fn http_error(what: &'static str, source: ureq::Error) -> Error {
    match source {
        ureq::Error::StatusCode(status) => Error::Status { what, status },
        source => Error::Http { what, source },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lumen() -> RepoName {
        "rsb/lumen".parse().unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn narrowed_request_names_one_repo_and_only_the_permissions_asked() {
        let request = TokenRequest::narrowed(
            &lumen(),
            Permissions {
                issues: Some(Access::Read),
                ..Permissions::default()
            },
        );
        let json = serde_json::to_string(&request).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            json,
            r#"{"repositories":["lumen"],"permissions":{"issues":"read"}}"#
        );
    }

    #[test]
    fn write_permissions_serialize_as_github_spells_them() {
        let request = TokenRequest::narrowed(
            &lumen(),
            Permissions {
                contents: Some(Access::Write),
                issues: Some(Access::Write),
                pull_requests: Some(Access::Write),
            },
        );
        let json = serde_json::to_value(&request).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            json,
            serde_json::json!({
                "repositories": ["lumen"],
                "permissions": {
                    "contents": "write",
                    "issues": "write",
                    "pull_requests": "write"
                }
            })
        );
    }

    #[test]
    fn empty_permissions_still_name_the_repo() {
        let request = TokenRequest::narrowed(&lumen(), Permissions::default());
        let json = serde_json::to_string(&request).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(json, r#"{"repositories":["lumen"],"permissions":{}}"#);
    }

    #[test]
    fn token_debug_redacts() {
        let minted: Minted =
            serde_json::from_str(r#"{"token":"ghs_secret","expires_at":"2026-10-17T00:00:00Z"}"#)
                .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(format!("{:?}", minted.token), "Token(<redacted>)");
        assert_eq!(minted.token.bearer(), "Bearer ghs_secret");
        assert_eq!(minted.expires_at, "2026-10-17T00:00:00Z");
    }

    #[test]
    fn an_edit_sends_only_the_body() {
        let body = EditBody {
            body: "- [x] a\r\n".to_owned(),
        };
        let json = serde_json::to_string(&body).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(json, r#"{"body":"- [x] a\r\n"}"#);
    }

    #[test]
    fn a_comment_sends_only_the_body() {
        let body = CommentBody {
            body: "Done.".to_owned(),
        };
        let json = serde_json::to_string(&body).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(json, r#"{"body":"Done."}"#);
    }

    #[test]
    fn list_query_encodes_the_label() {
        let query = ListQuery {
            state: "open",
            label: Some("good first issue&state=closed".to_owned()),
            per_page: 30,
        };
        assert_eq!(
            query.encode(),
            "state=open&per_page=30&labels=good%20first%20issue%26state%3Dclosed"
        );
        let plain = ListQuery {
            label: None,
            ..query
        };
        assert_eq!(plain.encode(), "state=open&per_page=30");
    }

    #[test]
    fn issues_and_pull_requests_parse_from_the_api_shape() {
        let text = r#"[
            {"number":3,"title":"Open a RAF","state":"open","body":null,
             "labels":[{"name":"ticket","color":"fff"}],
             "html_url":"https://github.com/rsb/lumen/issues/3","user":{"login":"rsb"}},
            {"number":4,"title":"A PR","state":"closed","body":"x","labels":[],
             "html_url":"https://github.com/rsb/lumen/pull/4",
             "pull_request":{"url":"https://api.github.com/repos/rsb/lumen/pulls/4"}}
        ]"#;
        let issues: Vec<Issue> = serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(issues.len(), 2);
        let (issue, pr) = (&issues[0], &issues[1]);
        assert_eq!(issue.state, State::Open);
        assert_eq!(issue.body, None);
        assert_eq!(issue.labels[0].name, "ticket");
        assert!(!issue.is_pull_request());
        assert_eq!(pr.state, State::Closed);
        assert!(pr.is_pull_request());
    }

    #[test]
    fn status_errors_do_not_carry_the_body() {
        let error = http_error("token mint", ureq::Error::StatusCode(401));
        assert_eq!(
            error.to_string(),
            "GitHub returned status 401 for token mint"
        );
    }
}
