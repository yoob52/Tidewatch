//! Skill 市场：扫描本机已装的 Agent Skill，从 skills.sh 搜索，并从 GitHub 安装到各 Agent 的 skills 目录。
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SEARCH_URL: &str = "https://skills.sh/api/search";
const SEED_QUERIES: &[&str] = &[
    "react", "design", "test", "git", "docs", "frontend", "python", "review",
];
const MAX_FILES: usize = 200;
const MAX_FILE_BYTES: usize = 512 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

/// 安装目标：每个 Agent 读取 skill 的用户级目录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTarget {
    ClaudeCode,
    Codex,
    Shared,
}

impl SkillTarget {
    pub const ALL: [SkillTarget; 3] = [
        SkillTarget::ClaudeCode,
        SkillTarget::Codex,
        SkillTarget::Shared,
    ];

    pub fn dir_in(self, home: &Path) -> PathBuf {
        match self {
            SkillTarget::ClaudeCode => home.join(".claude").join("skills"),
            SkillTarget::Codex => home.join(".codex").join("skills"),
            SkillTarget::Shared => home.join(".agents").join("skills"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub target: SkillTarget,
    pub dir: String,
    pub files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketSkill {
    pub id: String,
    pub source: String,
    #[serde(rename = "skillId")]
    pub skill_id: String,
    pub name: String,
    #[serde(default)]
    pub installs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillPreview {
    pub name: String,
    pub description: String,
    pub source: String,
    pub path: String,
    pub content: String,
    pub files: Vec<String>,
    /// 预览读取的提交；安装时按它下载，保证装的就是看过的内容。
    pub commit: String,
}

pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| Error::invalid("找不到用户主目录。"))
}

/// 解析 SKILL.md 开头的 YAML frontmatter，取 name / description。
pub fn parse_frontmatter(text: &str) -> (Option<String>, Option<String>) {
    let text = text.trim_start_matches('\u{feff}');
    let Some(rest) = text.strip_prefix("---") else {
        return (None, None);
    };
    let Some(end) = rest.find("\n---") else {
        return (None, None);
    };
    let (mut name, mut desc) = (None, None);
    let mut lines = rest[..end].lines().peekable();
    while let Some(line) = lines.next() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim();
        let mut v = v.trim().to_string();
        if matches!(v.as_str(), "|" | ">" | "|-" | ">-") {
            let mut parts = Vec::new();
            while let Some(next) = lines.peek() {
                if next.starts_with(' ') || next.starts_with('\t') || next.is_empty() {
                    parts.push(next.trim().to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            v = parts.join(" ").trim().to_string();
        }
        let v = if v.starts_with('"') {
            serde_json::from_str::<String>(&v).unwrap_or_else(|_| v.trim_matches('"').to_string())
        } else {
            v.trim_matches('\'').to_string()
        };
        match k {
            "name" if !v.is_empty() => name = Some(v),
            "description" if !v.is_empty() => desc = Some(v),
            _ => {}
        }
    }
    (name, desc)
}

/// skill 目录名只允许字母、数字、`-`、`_`、`.`，不能跳出 skills 目录。
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn count_files(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                count_files(&p)
            } else {
                1
            }
        })
        .sum()
}

pub fn list_installed_in(home: &Path) -> Vec<InstalledSkill> {
    let mut out = Vec::new();
    for target in SkillTarget::ALL {
        let root = target.dir_in(home);
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            let dir = e.path();
            let md = dir.join("SKILL.md");
            let Ok(text) = std::fs::read_to_string(&md) else {
                continue;
            };
            let (name, desc) = parse_frontmatter(&text);
            out.push(InstalledSkill {
                name: name.unwrap_or_else(|| e.file_name().to_string_lossy().into_owned()),
                description: desc.unwrap_or_default(),
                target,
                dir: dir.to_string_lossy().into_owned(),
                files: count_files(&dir),
            });
        }
    }
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

pub fn list_installed() -> Result<Vec<InstalledSkill>> {
    Ok(list_installed_in(&home_dir()?))
}

fn installed_dir(home: &Path, target: SkillTarget, dir_name: &str) -> Result<PathBuf> {
    if !valid_name(dir_name) {
        return Err(Error::invalid("skill 名称只能包含字母、数字、-、_ 和 .。"));
    }
    Ok(target.dir_in(home).join(dir_name))
}

pub fn remove_in(home: &Path, target: SkillTarget, dir_name: &str) -> Result<()> {
    let dir = installed_dir(home, target, dir_name)?;
    if !dir.join("SKILL.md").is_file() {
        return Err(Error::invalid("没有找到这个 skill。"));
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

pub fn remove(target: SkillTarget, dir_name: &str) -> Result<()> {
    remove_in(&home_dir()?, target, dir_name)
}

/// 新建一个只有 SKILL.md 的本地 skill。
pub fn create_in(
    home: &Path,
    targets: &[SkillTarget],
    name: &str,
    description: &str,
    body: &str,
) -> Result<()> {
    if targets.is_empty() {
        return Err(Error::invalid("至少选择一个安装目标。"));
    }
    let desc = description.trim().replace('\n', " ");
    if desc.is_empty() {
        return Err(Error::invalid(
            "请填写 skill 的描述，Agent 靠它判断何时使用。",
        ));
    }
    let content = format!(
        "---\nname: {name}\ndescription: {}\n---\n\n{}\n",
        serde_json::Value::String(desc),
        body.trim()
    );
    for t in targets {
        let dir = installed_dir(home, *t, name)?;
        if dir.exists() {
            return Err(Error::invalid(format!("{} 已存在。", dir.display())));
        }
    }
    for t in targets {
        let dir = installed_dir(home, *t, name)?;
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("SKILL.md"), &content)?;
    }
    Ok(())
}

pub fn create(targets: &[SkillTarget], name: &str, description: &str, body: &str) -> Result<()> {
    create_in(&home_dir()?, targets, name, description, body)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .into()
}

fn github_token() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
}

fn get_text(url: &str, accept: &str) -> Result<String> {
    let mut req = agent()
        .get(url)
        .header("User-Agent", "Tidewatch")
        .header("Accept", accept);
    if url.starts_with("https://api.github.com/") {
        if let Some(t) = github_token() {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
    }
    let mut resp = req
        .call()
        .map_err(|e| Error::invalid(format!("网络请求失败：{e}")))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(Error::invalid(format!(
            "请求 {url} 失败：服务器返回 {status}。"
        )));
    }
    resp.body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| Error::invalid(format!("读取响应失败：{e}")))
}

fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn parse_search(body: &str) -> Result<Vec<MarketSkill>> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(default)]
        skills: Vec<MarketSkill>,
    }
    let r: Resp = serde_json::from_str(body)
        .map_err(|e| Error::invalid(format!("市场返回的数据无法解析：{e}")))?;
    Ok(r.skills)
}

fn merge_sorted(lists: Vec<Vec<MarketSkill>>) -> Vec<MarketSkill> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<MarketSkill> = lists
        .into_iter()
        .flatten()
        .filter(|s| seen.insert(s.id.clone()))
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.installs));
    out
}

/// 搜索 skills.sh；空查询返回几组常用关键词的合并结果（按安装量排序）。
pub fn market_search(query: &str) -> Result<Vec<MarketSkill>> {
    let q = query.trim();
    if q.chars().count() >= 2 {
        let body = get_text(
            &format!("{SEARCH_URL}?q={}&limit=60", encode(q)),
            "application/json",
        )?;
        return Ok(merge_sorted(vec![parse_search(&body)?]));
    }
    let lists: Vec<Vec<MarketSkill>> = std::thread::scope(|s| {
        let handles: Vec<_> = SEED_QUERIES
            .iter()
            .map(|q| {
                s.spawn(move || {
                    get_text(&format!("{SEARCH_URL}?q={q}&limit=20"), "application/json")
                        .and_then(|b| parse_search(&b))
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().and_then(|r| r.ok()))
            .collect()
    });
    if lists.is_empty() {
        return Err(Error::invalid("连不上 skills.sh，请检查网络或代理。"));
    }
    Ok(merge_sorted(lists))
}

/// `owner/repo` 形式的来源才能从 GitHub 安装。
pub fn github_repo(source: &str) -> Option<(&str, &str)> {
    let (o, r) = source.split_once('/')?;
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (ok(o) && ok(r) && !o.contains('.')).then_some((o, r))
}

/// 在仓库文件树里找到 skill 目录：优先目录名等于 skillId 的 SKILL.md。
pub fn pick_skill_dir<'a>(paths: &'a [String], skill_id: &str) -> Vec<&'a str> {
    let mds: Vec<&str> = paths
        .iter()
        .map(String::as_str)
        .filter(|p| *p == "SKILL.md" || p.ends_with("/SKILL.md"))
        .collect();
    let dir_of = |p: &'a str| {
        p.strip_suffix("SKILL.md")
            .unwrap_or(p)
            .trim_end_matches('/')
    };
    let mut exact: Vec<&str> = mds
        .iter()
        .map(|p| dir_of(p))
        .filter(|d| d.rsplit('/').next() == Some(skill_id))
        .collect();
    if exact.is_empty() {
        return mds.into_iter().map(dir_of).collect();
    }
    exact.sort_by_key(|d| d.len());
    exact
}

struct Located {
    owner: String,
    repo: String,
    commit: String,
    dir: String,
    files: Vec<String>,
    content: String,
}

fn raw_url(owner: &str, repo: &str, commit: &str, path: &str) -> String {
    let path: Vec<String> = path.split('/').map(encode).collect();
    format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{commit}/{}",
        path.join("/")
    )
}

pub fn valid_commit(c: &str) -> bool {
    c.len() == 40 && c.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 从 git 智能协议的引用公告里取 HEAD 指向的提交。
pub fn parse_head_ref(text: &str) -> Option<String> {
    text.split('\n').find_map(|line| {
        let i = line
            .find(" HEAD\0")
            .or_else(|| line.strip_suffix(" HEAD").map(str::len))?;
        let sha = line.get(i.checked_sub(40)?..i)?;
        valid_commit(sha).then(|| sha.to_ascii_lowercase())
    })
}

/// 默认分支当前的提交。先走 git 协议（不占 GitHub API 配额），失败再问 API。
fn resolve_head(owner: &str, repo: &str) -> Result<String> {
    let via_git = get_text(
        &format!("https://github.com/{owner}/{repo}.git/info/refs?service=git-upload-pack"),
        "*/*",
    )
    .ok()
    .and_then(|t| parse_head_ref(&t));
    if let Some(c) = via_git {
        return Ok(c);
    }
    let sha = get_text(
        &format!("https://api.github.com/repos/{owner}/{repo}/commits/HEAD"),
        "application/vnd.github.sha",
    )?;
    let sha = sha.trim().to_ascii_lowercase();
    if valid_commit(&sha) {
        Ok(sha)
    } else {
        Err(Error::invalid(format!(
            "无法确定 {owner}/{repo} 的当前提交。"
        )))
    }
}

fn locate(source: &str, skill_id: &str, commit: Option<&str>) -> Result<Located> {
    let (owner, repo) = github_repo(source).ok_or_else(|| {
        Error::invalid(format!("「{source}」不是 GitHub 仓库，暂不支持一键安装。"))
    })?;
    let commit = match commit {
        Some(c) if valid_commit(c) => c.to_ascii_lowercase(),
        Some(c) => return Err(Error::invalid(format!("「{c}」不是有效的提交号。"))),
        None => resolve_head(owner, repo)?,
    };
    let blobs = list_files(owner, repo, &commit)?;
    for dir in pick_skill_dir(&blobs, skill_id).into_iter().take(30) {
        let md_path = if dir.is_empty() {
            "SKILL.md".to_string()
        } else {
            format!("{dir}/SKILL.md")
        };
        let Ok(content) = get_text(&raw_url(owner, repo, &commit, &md_path), "text/plain") else {
            continue;
        };
        let (name, _) = parse_frontmatter(&content);
        let dir_name = dir.rsplit('/').next().unwrap_or("");
        if dir_name != skill_id && name.as_deref() != Some(skill_id) {
            continue;
        }
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let files: Vec<String> = blobs
            .iter()
            .filter_map(|p| p.strip_prefix(prefix.as_str()).map(String::from))
            .filter(|p| !dir.is_empty() || !p.contains('/'))
            .collect();
        return Ok(Located {
            owner: owner.into(),
            repo: repo.into(),
            commit,
            dir: dir.to_string(),
            files,
            content,
        });
    }
    Err(Error::invalid(format!(
        "在 {source} 里没有找到 skill「{skill_id}」。"
    )))
}

/// 仓库里的全部文件路径。未登录的 GitHub API 每小时只有 60 次，超限时改用 jsDelivr 的文件列表。
fn list_files(owner: &str, repo: &str, commit: &str) -> Result<Vec<String>> {
    let gh = get_text(
        &format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{commit}?recursive=1"),
        "application/vnd.github+json",
    )
    .and_then(|t| parse_github_tree(&t));
    let Err(e) = gh else { return gh };
    let jsdelivr = |rev: &str| {
        get_text(
            &format!(
                "https://data.jsdelivr.com/v1/packages/gh/{owner}/{repo}@{rev}?structure=flat"
            ),
            "application/json",
        )
        .and_then(|t| parse_jsdelivr_files(&t))
    };
    // 大仓库按提交号列文件会超过 jsDelivr 的 50 MB 限制，@HEAD 不受限；提交仍是当前 HEAD 时才用它。
    if let Ok(files) = jsdelivr(commit) {
        return Ok(files);
    }
    match resolve_head(owner, repo) {
        Ok(head) if head == commit => jsdelivr("HEAD").map_err(|_| e),
        _ => Err(e),
    }
}

fn parse_json(text: &str) -> Result<serde_json::Value> {
    serde_json::from_str(text).map_err(|e| Error::invalid(format!("返回的数据无法解析：{e}")))
}

pub fn parse_github_tree(text: &str) -> Result<Vec<String>> {
    let v = parse_json(text)?;
    Ok(v["tree"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|e| e["type"] == "blob")
                .filter_map(|e| e["path"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

pub fn parse_jsdelivr_files(text: &str) -> Result<Vec<String>> {
    let v = parse_json(text)?;
    Ok(v["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["name"].as_str())
                .map(|n| n.trim_start_matches('/').to_string())
                .collect()
        })
        .unwrap_or_default())
}

pub fn preview(source: &str, skill_id: &str) -> Result<SkillPreview> {
    let l = locate(source, skill_id, None)?;
    let (name, desc) = parse_frontmatter(&l.content);
    Ok(SkillPreview {
        name: name.unwrap_or_else(|| skill_id.to_string()),
        description: desc.unwrap_or_default(),
        source: source.to_string(),
        path: l.dir,
        content: l.content,
        files: l.files,
        commit: l.commit,
    })
}

/// 按 `commit`（预览时看到的提交，缺省取默认分支当前提交）下载 skill 目录下的全部文件，
/// 安装到所选目标。已存在时覆盖（视为更新）。
pub fn install(
    source: &str,
    skill_id: &str,
    commit: Option<&str>,
    targets: &[SkillTarget],
) -> Result<Vec<String>> {
    install_in(&home_dir()?, source, skill_id, commit, targets)
}

const ORIGIN_FILE: &str = ".tidewatch-source";

/// 只覆盖从同一来源装的 skill；同名的本地 skill 或其他来源的不动。
fn check_overwrite(
    home: &Path,
    targets: &[SkillTarget],
    skill_id: &str,
    origin: &str,
) -> Result<()> {
    for t in targets {
        let dir = installed_dir(home, *t, skill_id)?;
        if !dir.exists() {
            continue;
        }
        let same = std::fs::read_to_string(dir.join(ORIGIN_FILE)).is_ok_and(|o| o.trim() == origin);
        if !same {
            return Err(Error::invalid(format!(
                "{} 已有同名 skill，且不是从 {origin} 安装的。请先删除或改名后再安装。",
                dir.display()
            )));
        }
    }
    Ok(())
}

fn install_in(
    home: &Path,
    source: &str,
    skill_id: &str,
    commit: Option<&str>,
    targets: &[SkillTarget],
) -> Result<Vec<String>> {
    if targets.is_empty() {
        return Err(Error::invalid("至少选择一个安装目标。"));
    }
    let origin = format!("{source}/{skill_id}");
    check_overwrite(home, targets, skill_id, &origin)?;
    let l = locate(source, skill_id, commit)?;
    if l.files.len() > MAX_FILES {
        return Err(Error::invalid(format!(
            "这个 skill 有 {} 个文件，超过上限 {MAX_FILES}。",
            l.files.len()
        )));
    }
    let mut files = Vec::new();
    let mut total = 0usize;
    for rel in &l.files {
        if rel.contains(['\\', ':'])
            || rel
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            continue;
        }
        let full = if l.dir.is_empty() {
            rel.clone()
        } else {
            format!("{}/{rel}", l.dir)
        };
        let mut resp = agent()
            .get(&raw_url(&l.owner, &l.repo, &l.commit, &full))
            .header("User-Agent", "Tidewatch")
            .call()
            .map_err(|e| Error::invalid(format!("下载 {rel} 失败：{e}")))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(Error::invalid(format!(
                "下载 {rel} 失败：服务器返回 {status}。"
            )));
        }
        let bytes = resp
            .body_mut()
            .with_config()
            .limit(MAX_FILE_BYTES as u64)
            .read_to_vec()
            .map_err(|e| Error::invalid(format!("下载 {rel} 失败：{e}")))?;
        total += bytes.len();
        if total > MAX_TOTAL_BYTES {
            return Err(Error::invalid(format!(
                "这个 skill 超过 {} MB，未安装。",
                MAX_TOTAL_BYTES / 1024 / 1024
            )));
        }
        files.push((rel.clone(), bytes));
    }
    let mut dirs = Vec::new();
    for t in targets {
        let dir = installed_dir(home, *t, skill_id)?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        for (rel, bytes) in &files {
            let p = dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&p, bytes)?;
        }
        std::fs::write(dir.join(ORIGIN_FILE), &origin)?;
        dirs.push(dir.to_string_lossy().into_owned());
    }
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_parses_plain_and_block() {
        let (n, d) = parse_frontmatter("---\nname: pdf\ndescription: \"Read PDFs\"\n---\nbody");
        assert_eq!(n.as_deref(), Some("pdf"));
        assert_eq!(d.as_deref(), Some("Read PDFs"));
        let (_, d) = parse_frontmatter(
            "---\nname: x\ndescription: >\n  line one\n  line two\nlicense: MIT\n---\n",
        );
        assert_eq!(d.as_deref(), Some("line one line two"));
        assert_eq!(parse_frontmatter("# no fm"), (None, None));
    }

    #[test]
    fn names_cannot_escape() {
        assert!(valid_name("vercel-react_1.0"));
        for bad in ["", "..", "../x", "a/b", ".hidden", "a b"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn github_sources() {
        assert_eq!(
            github_repo("vercel-labs/agent-skills"),
            Some(("vercel-labs", "agent-skills"))
        );
        assert_eq!(github_repo("open.feishu.cn"), None);
        assert_eq!(github_repo("a/b/c"), None);
    }

    #[test]
    fn picks_matching_dir_first() {
        let paths: Vec<String> = [
            "README.md",
            "skills/a/SKILL.md",
            "skills/react/SKILL.md",
            "x/react/SKILL.md",
            "skills/react/ref.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            pick_skill_dir(&paths, "react"),
            vec!["x/react", "skills/react"]
        );
        assert_eq!(
            pick_skill_dir(&paths, "zzz"),
            vec!["skills/a", "skills/react", "x/react"]
        );
        assert_eq!(pick_skill_dir(&["SKILL.md".to_string()], "solo"), vec![""]);
    }

    #[test]
    fn file_lists_from_github_and_jsdelivr() {
        let gh = r#"{"tree":[{"path":"skills/a/SKILL.md","type":"blob"},{"path":"skills/a","type":"tree"}]}"#;
        assert_eq!(parse_github_tree(gh).unwrap(), vec!["skills/a/SKILL.md"]);
        let jd = r#"{"files":[{"name":"/skills/a/SKILL.md","size":3},{"name":"/README.md"}]}"#;
        assert_eq!(
            parse_jsdelivr_files(jd).unwrap(),
            vec!["skills/a/SKILL.md", "README.md"]
        );
    }

    #[test]
    fn head_ref_from_git_advertisement() {
        let sha = "0123456789abcdef0123456789ABCDEF01234567";
        let adv = format!(
            "001e# service=git-upload-pack\n0000015b{sha} HEAD\0multi_ack symref=HEAD:refs/heads/main\n003f{sha} refs/heads/main\n0000"
        );
        assert_eq!(parse_head_ref(&adv), Some(sha.to_ascii_lowercase()));
        assert_eq!(parse_head_ref("001e# service=git-upload-pack\n0000"), None);
        assert!(valid_commit(&sha[..]));
        assert!(!valid_commit("HEAD"));
        assert!(!valid_commit("../../etc/passwd/0123456789abcdef01234567"));
        assert_eq!(
            raw_url("o", "r", "abc", "a b/SKILL.md"),
            "https://raw.githubusercontent.com/o/r/abc/a%20b/SKILL.md"
        );
    }

    #[test]
    fn install_never_overwrites_foreign_skills() {
        let home = tempfile::tempdir().unwrap();
        let t = [SkillTarget::ClaudeCode];
        create_in(home.path(), &t, "pdf", "a: b", "body").unwrap();
        assert!(check_overwrite(home.path(), &t, "pdf", "o/r/pdf").is_err());
        let dir = SkillTarget::ClaudeCode.dir_in(home.path()).join("pdf");
        std::fs::write(dir.join(ORIGIN_FILE), "o/r/pdf").unwrap();
        assert!(check_overwrite(home.path(), &t, "pdf", "o/r/pdf").is_ok());
        assert!(check_overwrite(home.path(), &t, "new", "o/r/new").is_ok());
        let md = std::fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert_eq!(parse_frontmatter(&md).1.as_deref(), Some("a: b"));
    }

    #[test]
    fn search_parses_and_merges() {
        let body = r#"{"skills":[{"id":"o/r/a","source":"o/r","skillId":"a","name":"a","installs":5},{"id":"o/r/b","source":"o/r","skillId":"b","name":"b","installs":9}]}"#;
        let a = parse_search(body).unwrap();
        let merged = merge_sorted(vec![a.clone(), a]);
        assert_eq!(
            merged.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"]
        );
    }

    #[test]
    fn create_list_remove_roundtrip() {
        let home = tempfile::tempdir().unwrap();
        create_in(
            home.path(),
            &[SkillTarget::ClaudeCode, SkillTarget::Shared],
            "my-skill",
            "Does things",
            "Steps",
        )
        .unwrap();
        let list = list_installed_in(home.path());
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].description, "Does things");
        assert!(create_in(home.path(), &[SkillTarget::ClaudeCode], "my-skill", "d", "").is_err());
        assert!(create_in(home.path(), &[SkillTarget::Codex], "../evil", "d", "").is_err());
        remove_in(home.path(), SkillTarget::ClaudeCode, "my-skill").unwrap();
        assert_eq!(list_installed_in(home.path()).len(), 1);
        assert!(remove_in(home.path(), SkillTarget::ClaudeCode, "my-skill").is_err());
    }
}
