//! GitHub Issues：按仓库和 label 拉取，评论进时间线，可选在验收后回写评论并关闭。
use serde_json::{json, Value};

use super::http;
use crate::{Error, Result};

pub const DEFAULT_API: &str = "https://api.github.com";
/// 一次同步最多翻这么多页（每页 100 条）。
const MAX_PAGES: usize = 5;
/// 一次同步最多给这么多个 Issue 拉评论，免得第一次同步把额度用完。
const MAX_COMMENT_FETCHES: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    /// open | closed
    pub state: String,
    pub url: String,
    pub labels: Vec<String>,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub url: String,
}

impl Issue {
    /// 标题和正文的指纹：只有它变了才算"源已变更"（评论、label 的变化不算）。
    pub fn rev(&self) -> String {
        crate::util::sha256_hex(format!("{}\n{}", self.title, self.body).as_bytes())[..16]
            .to_string()
    }
}

/// 一次拉取的结果。
pub struct Fetched {
    pub issues: Vec<Issue>,
    /// 没拉完时：下次用它做 `since` 接着拉。
    pub resume_since: Option<String>,
}

pub struct Client<'a> {
    pub api: &'a str,
    /// `owner/name`
    pub repo: &'a str,
    pub token: Option<&'a str>,
}

impl Client<'_> {
    fn headers(&self) -> Vec<(&'static str, String)> {
        let mut h = vec![
            ("Accept", "application/vnd.github+json".to_string()),
            ("X-GitHub-Api-Version", "2022-11-28".to_string()),
        ];
        if let Some(t) = self.token {
            h.push(("Authorization", format!("Bearer {t}")));
        }
        h
    }

    fn base(&self) -> String {
        format!("{}/repos/{}", self.api.trim_end_matches('/'), self.repo)
    }

    fn check(&self, resp: &http::Response, what: &str) -> Result<()> {
        if resp.ok() {
            return Ok(());
        }
        let message = resp.json()["message"].as_str().unwrap_or("").to_string();
        let hint = match resp.status {
            401 => "令牌无效或已过期。",
            403 | 429 => "没有权限，或请求次数超限（没填令牌时每小时只有 60 次）。",
            404 => "仓库不存在，或令牌看不到它（私有仓库要填有权限的令牌）。",
            _ => "",
        };
        Err(Error::invalid(format!(
            "{what}失败：GitHub 返回 {} {message} {hint}",
            resp.status
        )))
    }

    /// 拉 Issue。第一次（`since` 为空）只拉还开着的；之后拉 `since` 以来有更新的全部状态。
    ///
    /// 按更新时间从旧到新拉。一次拉不完（页数或评论次数到了上限）就停在那里，`resume_since`
    /// 是第一条没处理的 Issue 的更新时间，下次从它接着拉。
    ///
    /// `comments_since` 单独给：接着拉的时候 Issue 从断点开始，评论仍然要从上次完整同步算起。
    pub fn issues(
        &self,
        labels: &[String],
        since: Option<&str>,
        comments_since: Option<&str>,
    ) -> Result<Fetched> {
        let mut out = vec![];
        let mut budget = MAX_COMMENT_FETCHES;
        let stop = |issues, at: &Value| {
            Ok(Fetched {
                issues,
                resume_since: at["updated_at"].as_str().map(String::from),
            })
        };
        for page in 1..=MAX_PAGES {
            let mut url = format!(
                "{}/issues?per_page=100&page={page}&sort=updated&direction=asc&state={}",
                self.base(),
                if since.is_some() { "all" } else { "open" }
            );
            if !labels.is_empty() {
                url.push_str(&format!("&labels={}", http::enc(&labels.join(","))));
            }
            if let Some(s) = since {
                url.push_str(&format!("&since={}", http::enc(s)));
            }
            let resp = http::get(&url, &self.headers())?;
            self.check(&resp, "拉取 Issue ")?;
            let list = resp.json();
            let items = list.as_array().cloned().unwrap_or_default();
            let n = items.len();
            for item in &items {
                // 这个接口把 PR 也当 Issue 返回
                if item.get("pull_request").is_some() {
                    continue;
                }
                let mut issue = parse_issue(item);
                if item["comments"].as_u64().unwrap_or(0) > 0 {
                    if budget == 0 {
                        return stop(out, item);
                    }
                    budget -= 1;
                    issue.comments = self.comments(issue.number, comments_since)?;
                }
                out.push(issue);
            }
            if n < 100 {
                break;
            }
            if page == MAX_PAGES {
                return stop(out, &items[n - 1]);
            }
        }
        Ok(Fetched {
            issues: out,
            resume_since: None,
        })
    }

    fn comments(&self, number: u64, since: Option<&str>) -> Result<Vec<Comment>> {
        let mut url = format!("{}/issues/{number}/comments?per_page=100", self.base());
        if let Some(s) = since {
            url.push_str(&format!("&since={}", http::enc(s)));
        }
        let resp = http::get(&url, &self.headers())?;
        self.check(&resp, "拉取评论")?;
        Ok(resp
            .json()
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|c| Comment {
                        id: c["id"].as_u64().unwrap_or(0),
                        author: c["user"]["login"].as_str().unwrap_or("").to_string(),
                        body: c["body"].as_str().unwrap_or("").to_string(),
                        url: c["html_url"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// 回写：评论一条，然后把 Issue 关掉。
    pub fn comment_and_close(&self, number: u64, comment: &str) -> Result<()> {
        if self.token.is_none() {
            return Err(Error::invalid(
                "回写 GitHub 需要令牌：在同步来源里填一个对这个仓库有 Issues 写权限的令牌，或在本机登录 gh。",
            ));
        }
        let resp = http::send(
            "POST",
            &format!("{}/issues/{number}/comments", self.base()),
            &self.headers(),
            &json!({ "body": comment }),
        )?;
        self.check(&resp, "回写评论")?;
        let resp = http::send(
            "PATCH",
            &format!("{}/issues/{number}", self.base()),
            &self.headers(),
            &json!({ "state": "closed", "state_reason": "completed" }),
        )?;
        self.check(&resp, "关闭 Issue ")
    }
}

fn parse_issue(v: &Value) -> Issue {
    Issue {
        number: v["number"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or("").trim().to_string(),
        body: v["body"].as_str().unwrap_or("").replace("\r\n", "\n"),
        state: v["state"].as_str().unwrap_or("open").to_string(),
        url: v["html_url"].as_str().unwrap_or("").to_string(),
        labels: v["labels"]
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(|x| x["name"].as_str().or(x.as_str()))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
        comments: vec![],
    }
}

/// `owner/name`，也接受整条仓库地址。
pub fn parse_repo(input: &str) -> Result<String> {
    let s = input
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit("github.com/")
        .next()
        .unwrap_or("")
        .trim_start_matches("git@github.com:");
    let parts: Vec<&str> = s.split('/').collect();
    let ok = |p: &str| {
        !p.is_empty()
            && p.chars().any(|c| c != '.')
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if parts.len() == 2 && ok(parts[0]) && ok(parts[1]) {
        Ok(format!("{}/{}", parts[0], parts[1]))
    } else {
        Err(Error::invalid(
            "仓库要写成 owner/name，例如 yoob52/Tidewatch。",
        ))
    }
}

/// 本机 `gh` 的登录令牌（没装或没登录就是 `None`）。
pub fn token_from_gh() -> Option<String> {
    let gh = crate::git::gh_binary()?;
    let mut cmd = std::process::Command::new(gh);
    cmd.args(["auth", "token"])
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        // 不弹控制台窗口
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !token.is_empty()).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_accepts_slug_and_urls() {
        for s in [
            "yoob52/Tidewatch",
            " https://github.com/yoob52/Tidewatch/ ",
            "https://github.com/yoob52/Tidewatch.git",
            "git@github.com:yoob52/Tidewatch.git",
        ] {
            assert_eq!(parse_repo(s).unwrap(), "yoob52/Tidewatch", "{s}");
        }
        for s in ["", "yoob52", "a/b/c", "a b/c", "../x"] {
            assert!(parse_repo(s).is_err(), "{s}");
        }
    }

    #[test]
    fn rev_follows_title_and_body_only() {
        let v = json!({ "number": 3, "title": " 崩溃 ", "body": "a\r\nb", "state": "open",
            "html_url": "https://github.com/o/r/issues/3", "labels": [{ "name": "bug" }], "comments": 2 });
        let a = parse_issue(&v);
        assert_eq!((a.title.as_str(), a.body.as_str()), ("崩溃", "a\nb"));
        assert_eq!(a.labels, ["bug"]);
        let mut b = a.clone();
        b.state = "closed".into();
        b.labels.clear();
        assert_eq!(a.rev(), b.rev());
        b.body.push('!');
        assert_ne!(a.rev(), b.rev());
    }
}
