//! Git：直接调用系统 `git`。每个子任务一个分支 + 一个 worktree。
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

fn base_cmd(cwd: &Path) -> Command {
    let mut c = Command::new("git");
    // Windows 上 worktree 嵌套较深，统一开启长路径；其它平台该配置无影响。
    c.args(["-c", "core.longpaths=true", "-c", "core.quotepath=false"]);
    c.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

/// 运行 git，成功返回 stdout（去尾部换行）。
pub fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = base_cmd(cwd)
        .args(args)
        .output()
        .map_err(|e| Error::Git(format!("无法启动 git：{e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = if err.trim().is_empty() {
            String::from_utf8_lossy(&out.stdout).to_string()
        } else {
            err.to_string()
        };
        return Err(Error::Git(format!(
            "git {}：{}",
            args.join(" "),
            msg.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    base_cmd(cwd)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn version() -> Option<String> {
    let out = Command::new("git").arg("--version").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn is_repo(path: &Path) -> bool {
    path.is_dir() && git_ok(path, &["rev-parse", "--is-inside-work-tree"])
}

pub fn current_branch(repo: &Path) -> Result<String> {
    git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
}

pub fn rev_parse(repo: &Path, rev: &str) -> Result<String> {
    git(
        repo,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
    )
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git_ok(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: String,
}

/// 从 `base` 拉出新分支并创建 worktree；分支已存在则直接检出。
pub fn worktree_add(repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let p = path.to_string_lossy();
    if branch_exists(repo, branch) {
        git(repo, &["worktree", "add", &p, branch])?;
    } else {
        git(repo, &["worktree", "add", "-b", branch, &p, base])?;
    }
    Ok(())
}

pub fn worktree_remove(repo: &Path, path: &Path, force: bool) -> Result<()> {
    let p = path.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&p);
    let res = git(repo, &args);
    if res.is_err() && force && path.exists() && is_linked_worktree(repo, path) {
        // 文件被占用等情况：手动删目录再 prune。只对登记过的附加 worktree 这么做，绝不删主检出或无关目录。
        std::fs::remove_dir_all(path)?;
        git(repo, &["worktree", "prune"])?;
        return Ok(());
    }
    res.map(|_| ())
}

/// `path` 是否是 `repo` 登记过的附加 worktree（列表第一项是主检出，不算）。
fn is_linked_worktree(repo: &Path, path: &Path) -> bool {
    let Ok(target) = crate::util::canonical(path) else {
        return false;
    };
    worktree_list(repo).is_ok_and(|list| {
        list.iter()
            .skip(1)
            .any(|w| crate::util::canonical(&w.path).is_ok_and(|p| p == target))
    })
}

pub fn worktree_list(repo: &Path) -> Result<Vec<Worktree>> {
    let out = git(repo, &["worktree", "list", "--porcelain"])?;
    let mut list = vec![];
    let mut cur: Option<Worktree> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                list.push(w);
            }
            cur = Some(Worktree {
                path: PathBuf::from(p),
                branch: None,
                head: String::new(),
            });
        } else if let (Some(h), Some(w)) = (line.strip_prefix("HEAD "), cur.as_mut()) {
            w.head = h.to_string();
        } else if let (Some(b), Some(w)) = (line.strip_prefix("branch refs/heads/"), cur.as_mut()) {
            w.branch = Some(b.to_string());
        }
    }
    list.extend(cur);
    Ok(list)
}

pub fn delete_branch(repo: &Path, branch: &str, force: bool) -> Result<()> {
    git(repo, &["branch", if force { "-D" } else { "-d" }, branch]).map(|_| ())
}

pub fn is_dirty(worktree: &Path) -> Result<bool> {
    Ok(!git(worktree, &["status", "--porcelain"])?.is_empty())
}

/// 提交全部改动；没有改动返回 `None`。
pub fn commit_all(worktree: &Path, message: &str) -> Result<Option<String>> {
    git(worktree, &["add", "-A"])?;
    if !is_dirty(worktree)? {
        return Ok(None);
    }
    git(
        worktree,
        &[
            "-c",
            "user.name=Tidewatch",
            "-c",
            "user.email=tidewatch@localhost",
            "commit",
            "-q",
            "--no-verify",
            "-m",
            message,
        ],
    )?;
    Ok(Some(rev_parse(worktree, "HEAD")?))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileStat {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// `base...head` 的逐文件增删行数（head 为空则对比工作区）。
pub fn diff_stat(repo: &Path, base: &str, head: Option<&str>) -> Result<Vec<FileStat>> {
    let range = head.map(|h| format!("{base}...{h}"));
    let out = git(
        repo,
        &["diff", "--numstat", range.as_deref().unwrap_or(base)],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let (a, r, p) = (it.next()?, it.next()?, it.next()?);
            Some(FileStat {
                path: p.to_string(),
                added: a.parse().unwrap_or(0),
                removed: r.parse().unwrap_or(0),
            })
        })
        .collect())
}

pub fn diff(repo: &Path, base: &str, head: Option<&str>) -> Result<String> {
    let range = head.map(|h| format!("{base}...{h}"));
    git(
        repo,
        &["diff", "--no-color", range.as_deref().unwrap_or(base)],
    )
}

/// 在 `repo` 当前分支上合并 `branch`。冲突时中止合并并返回错误。
/// 要求已跟踪文件没有未提交改动：失败后的回退（`reset --merge`）会丢掉暂存区里的内容。
pub fn merge(repo: &Path, branch: &str, squash: bool, message: &str) -> Result<String> {
    if !git(repo, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err(Error::invalid(
            "目标分支有未提交的改动，请先提交或暂存（stash）后再合并",
        ));
    }
    rev_parse(repo, branch)?;
    let ident = [
        "-c",
        "user.name=Tidewatch",
        "-c",
        "user.email=tidewatch@localhost",
    ];
    let res = if squash {
        git(repo, &["merge", "--squash", branch]).and_then(|_| {
            let mut a = ident.to_vec();
            a.extend(["commit", "-q", "--no-verify", "-m", message]);
            git(repo, &a)
        })
    } else {
        let mut a = ident.to_vec();
        a.extend(["merge", "--no-ff", "--no-verify", "-m", message, branch]);
        git(repo, &a)
    };
    if let Err(e) = res {
        let _ = git(repo, &["merge", "--abort"]);
        let _ = git(repo, &["reset", "--merge"]);
        return Err(e);
    }
    rev_parse(repo, "HEAD")
}

/// 检查点：把某个提交记到 `refs/tidewatch/checkpoints/...`，分支被改写后仍可找回。
/// 仓库配置的远程名；优先 origin。
pub fn default_remote(repo: &Path) -> Result<Option<String>> {
    let all = git(repo, &["remote"])?;
    let mut names = all.lines().map(str::trim).filter(|l| !l.is_empty());
    Ok(if all.lines().any(|l| l.trim() == "origin") {
        Some("origin".into())
    } else {
        names.next().map(String::from)
    })
}

/// 把分支推到远程并设为上游。不强推：远程分支有本地没有的提交时会失败。
pub fn push_branch(repo: &Path, remote: &str, branch: &str) -> Result<()> {
    git(
        repo,
        &[
            "push",
            "--set-upstream",
            remote,
            &format!("refs/heads/{branch}:refs/heads/{branch}"),
        ],
    )
    .map(|_| ())
}

/// `gh` 的位置：环境变量 `TIDEWATCH_GH_BIN` 优先，否则在 PATH 里找。
pub fn gh_binary() -> Option<PathBuf> {
    std::env::var_os("TIDEWATCH_GH_BIN")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| which::which("gh").ok())
}

/// 用 GitHub CLI 开 PR，返回 PR 的地址。
pub fn gh_pr_create(
    repo: &Path,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
    draft: bool,
) -> Result<String> {
    let gh = gh_binary().ok_or_else(|| {
        Error::invalid("没有找到 GitHub CLI（gh）。安装并运行 gh auth login 之后再开 PR。")
    })?;
    let mut c = Command::new(gh);
    c.current_dir(repo)
        .args([
            "pr", "create", "--head", head, "--base", base, "--title", title, "--body", body,
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");
    if draft {
        c.arg("--draft");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c
        .output()
        .map_err(|e| Error::Git(format!("无法启动 gh：{e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // 已经有 PR 时 gh 报错并在 stderr 里给出地址：当作成功
    let url = |text: &str| {
        text.split_whitespace()
            .rev()
            .find(|w| w.starts_with("https://") && w.contains("/pull/"))
            .map(String::from)
    };
    if out.status.success() {
        return url(&stdout)
            .ok_or_else(|| Error::Git(format!("gh 没有返回 PR 地址：{}", stdout.trim())));
    }
    if stderr.contains("already exists") {
        if let Some(u) = url(&stderr) {
            return Ok(u);
        }
    }
    Err(Error::Git(format!("gh pr create：{}", stderr.trim())))
}

pub fn save_checkpoint(repo: &Path, name: &str, rev: &str) -> Result<String> {
    let r = format!("refs/tidewatch/checkpoints/{name}");
    git(repo, &["update-ref", &r, rev])?;
    Ok(r)
}

/// 长路径是否可用。Windows 需要 git 的 `core.longpaths`（本模块每次调用都带）以及系统层面的长路径策略。
pub fn long_paths_hint() -> Option<&'static str> {
    if cfg!(windows) {
        Some("worktree 路径可能超过 260 字符：观澜调用 git 时已带 core.longpaths=true；若仍失败，请在系统中启用长路径（LongPathsEnabled）或把工作区放在更短的目录。")
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn init_repo(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]).unwrap();
        std::fs::write(dir.join("README.md"), "# demo\n").unwrap();
        git(dir, &["add", "-A"]).unwrap();
        git(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "init",
            ],
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::init_repo;
    use super::*;

    #[test]
    fn worktree_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        assert!(is_repo(&repo));
        assert_eq!(current_branch(&repo).unwrap(), "main");

        let wt = dir.path().join("worktrees/demo/TW-1-frontend");
        worktree_add(&repo, &wt, "tw/TW-1-frontend", "main").unwrap();
        assert!(wt.join("README.md").exists());
        let listed = worktree_list(&repo).unwrap();
        assert!(listed
            .iter()
            .any(|w| w.branch.as_deref() == Some("tw/TW-1-frontend")));

        std::fs::write(wt.join("a.txt"), "hello\n").unwrap();
        let sha = commit_all(&wt, "feat: a").unwrap().expect("commit");
        assert_eq!(commit_all(&wt, "noop").unwrap(), None);
        let stat = diff_stat(&repo, "main", Some("tw/TW-1-frontend")).unwrap();
        assert_eq!(
            stat,
            vec![FileStat {
                path: "a.txt".into(),
                added: 1,
                removed: 0
            }]
        );
        assert!(diff(&repo, "main", Some("tw/TW-1-frontend"))
            .unwrap()
            .contains("+hello"));

        let cp = save_checkpoint(&repo, "task1/run1", &sha).unwrap();
        assert_eq!(git(&repo, &["rev-parse", &cp]).unwrap(), sha);

        merge(&repo, "tw/TW-1-frontend", false, "merge TW-1").unwrap();
        assert!(repo.join("a.txt").exists());

        worktree_remove(&repo, &wt, true).unwrap();
        assert!(!wt.exists());
        delete_branch(&repo, "tw/TW-1-frontend", false).unwrap();
        assert!(!branch_exists(&repo, "tw/TW-1-frontend"));
    }

    #[test]
    fn merge_conflict_is_aborted() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let wt = dir.path().join("wt");
        worktree_add(&repo, &wt, "tw/x", "main").unwrap();
        std::fs::write(wt.join("README.md"), "# branch\n").unwrap();
        commit_all(&wt, "branch").unwrap();
        std::fs::write(repo.join("README.md"), "# main\n").unwrap();
        commit_all(&repo, "main").unwrap();
        assert!(merge(&repo, "tw/x", false, "m").is_err());
        assert!(!is_dirty(&repo).unwrap(), "冲突后应恢复干净");
    }

    #[test]
    fn failed_merge_keeps_staged_work() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        std::fs::write(repo.join("README.md"), "# staged\n").unwrap();
        git(&repo, &["add", "README.md"]).unwrap();

        assert!(merge(&repo, "no-such-branch", false, "m").is_err());
        assert!(merge(&repo, "main", true, "m").is_err());
        assert_eq!(
            git(&repo, &["diff", "--cached", "--name-only"]).unwrap(),
            "README.md"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("README.md")).unwrap(),
            "# staged\n"
        );
    }

    #[test]
    fn force_remove_never_deletes_non_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let other = dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("keep.txt"), "x").unwrap();

        assert!(worktree_remove(&repo, &repo, true).is_err());
        assert!(repo.join("README.md").exists(), "主检出不能被删");
        assert!(worktree_remove(&repo, &other, true).is_err());
        assert!(other.join("keep.txt").exists(), "无关目录不能被删");
    }

    /// 长路径：worktree 内的文件全路径超过 260 字符时，创建 / 提交 / 清理都应成功。
    #[test]
    fn long_paths_in_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        let wt = dir
            .path()
            .join("worktrees")
            .join("p".repeat(40))
            .join("TW-123-frontend");
        worktree_add(&repo, &wt, "tw/long", "main").unwrap();

        let mut deep = wt.clone();
        for _ in 0..6 {
            deep = deep.join("d".repeat(40));
        }
        let file = deep.join("component.tsx");
        assert!(file.to_string_lossy().len() > 260);
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(&file, "export {}\n").unwrap();
        commit_all(&wt, "long path").unwrap().expect("commit");
        assert_eq!(diff_stat(&repo, "main", Some("tw/long")).unwrap().len(), 1);
        worktree_remove(&repo, &wt, true).unwrap();
        assert!(!wt.exists());
    }
}
