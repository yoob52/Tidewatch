//! Windows 上 Agent 跑命令用哪个 Shell（设置里的「Agent 使用的 Shell」）。
//!
//! - Codex 只用 PowerShell：PATH 上有 `pwsh.exe` 就用它，没有就用系统自带的 `powershell.exe`，
//!   不读 `SHELL` / `COMSPEC`。所以这里靠改这次运行的 PATH 来选。
//! - Claude Code 默认用 Git Bash（`CLAUDE_CODE_GIT_BASH_PATH` 指定），
//!   `CLAUDE_CODE_USE_POWERSHELL_TOOL=1` 再给它一个 PowerShell 工具。
//! - 两个都不能把 cmd 当 Shell。
//!
//! Microsoft Store 版 PowerShell 在 PATH 上只是 WindowsApps 里的别名，Agent 的沙箱启动它会报
//! 「拒绝访问」，所以任何选项下都不让 Agent 找到这个别名。
use crate::agent::AgentKind;
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

pub const SHELLS: &[&str] = &["auto", "pwsh", "powershell", "git_bash"];

pub fn validate(choice: &str) -> Result<()> {
    if SHELLS.contains(&choice) {
        Ok(())
    } else {
        Err(Error::invalid(format!("未知的 Shell：{choice}")))
    }
}

/// 这次运行要追加（覆盖继承值）的环境变量。只在 Windows 上生效。
pub fn env_for(kind: AgentKind, choice: &str) -> Result<Vec<(String, String)>> {
    if !cfg!(windows) {
        return Ok(vec![]);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let real_pwsh = program_files
        .as_ref()
        .map(|p| p.join("PowerShell").join("7"))
        .filter(|d| d.join("pwsh.exe").is_file());
    plan(
        kind,
        choice,
        &path,
        &|p| p.is_file(),
        real_pwsh,
        git_bash(&path),
    )
}

/// 找 Git for Windows 的 bash.exe：常见安装位置，或者 PATH 上 git.exe 旁边的 `..\bin\bash.exe`。
fn git_bash(path: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join("Git").join("bin").join("bash.exe"))
        .collect();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(l).join(r"Programs\Git\bin\bash.exe"));
    }
    for dir in path.split(';').filter(|d| !d.is_empty()) {
        let dir = Path::new(dir);
        if dir.join("git.exe").is_file() {
            if let Some(root) = dir.parent() {
                candidates.push(root.join("bin").join("bash.exe"));
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn is_store_alias_dir(dir: &str) -> bool {
    dir.trim_end_matches(['/', '\\'])
        .to_ascii_lowercase()
        .replace('/', "\\")
        .ends_with(r"\microsoft\windowsapps")
}

pub(crate) fn plan(
    kind: AgentKind,
    choice: &str,
    path: &str,
    exists: &dyn Fn(&Path) -> bool,
    real_pwsh_dir: Option<PathBuf>,
    git_bash: Option<PathBuf>,
) -> Result<Vec<(String, String)>> {
    validate(choice)?;
    if !matches!(kind, AgentKind::Codex | AgentKind::ClaudeCode) {
        return Ok(vec![]);
    }
    let has_pwsh = |d: &str| exists(&Path::new(d).join("pwsh.exe"));
    let mut dirs: Vec<String> = path
        .split(';')
        .filter(|d| !d.is_empty())
        .filter(|d| !(is_store_alias_dir(d) && has_pwsh(d)))
        .map(String::from)
        .collect();
    let mut env = vec![];
    match choice {
        "pwsh" => {
            if !dirs.iter().any(|d| has_pwsh(d)) {
                let Some(d) = real_pwsh_dir else {
                    return Err(Error::invalid(
                        "没找到 PowerShell 7。Microsoft Store 版的 pwsh 不能被 Agent 启动，请用 MSI 安装 PowerShell 7，或改选 Windows PowerShell。",
                    ));
                };
                dirs.insert(0, d.display().to_string());
            }
        }
        "powershell" => dirs.retain(|d| !has_pwsh(d)),
        _ => {}
    }
    if kind == AgentKind::ClaudeCode {
        match choice {
            "pwsh" | "powershell" => {
                env.push(("CLAUDE_CODE_USE_POWERSHELL_TOOL".into(), "1".into()));
            }
            "git_bash" => {
                let Some(b) = git_bash else {
                    return Err(Error::invalid(
                        "没找到 Git Bash。请安装 Git for Windows，或改选其他 Shell。",
                    ));
                };
                env.push(("CLAUDE_CODE_GIT_BASH_PATH".into(), b.display().to_string()));
            }
            _ => {}
        }
    }
    let joined = dirs.join(";");
    if joined
        != path
            .split(';')
            .filter(|d| !d.is_empty())
            .collect::<Vec<_>>()
            .join(";")
    {
        env.push(("PATH".into(), joined));
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STORE: &str = r"C:\Users\u\AppData\Local\Microsoft\WindowsApps";
    const PS7: &str = r"C:\Program Files\PowerShell\7";

    fn with_pwsh<'a>(dirs: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
        move |p: &Path| dirs.iter().any(|d| p == Path::new(d).join("pwsh.exe"))
    }

    fn path_of(env: &[(String, String)]) -> Option<&str> {
        env.iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn hides_the_store_pwsh_alias() {
        let path = format!(r"C:\Windows\system32;{STORE}");
        let env = plan(
            AgentKind::Codex,
            "auto",
            &path,
            &with_pwsh(&[STORE]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(path_of(&env), Some(r"C:\Windows\system32"));
        // 别名目录里没有 pwsh 就不动 PATH
        let env = plan(AgentKind::Codex, "auto", &path, &with_pwsh(&[]), None, None).unwrap();
        assert!(env.is_empty());
    }

    #[test]
    fn powershell_hides_every_pwsh() {
        let path = format!(r"{PS7};C:\Windows\system32");
        let env = plan(
            AgentKind::Codex,
            "powershell",
            &path,
            &with_pwsh(&[PS7]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(path_of(&env), Some(r"C:\Windows\system32"));
        let env = plan(
            AgentKind::ClaudeCode,
            "powershell",
            &path,
            &with_pwsh(&[PS7]),
            None,
            None,
        )
        .unwrap();
        assert!(env.contains(&("CLAUDE_CODE_USE_POWERSHELL_TOOL".into(), "1".into())));
    }

    #[test]
    fn pwsh_needs_a_real_install() {
        let path = format!(r"C:\Windows\system32;{STORE}");
        let only_store = with_pwsh(&[STORE]);
        assert!(plan(AgentKind::Codex, "pwsh", &path, &only_store, None, None).is_err());
        let env = plan(
            AgentKind::Codex,
            "pwsh",
            &path,
            &only_store,
            Some(PS7.into()),
            None,
        )
        .unwrap();
        assert_eq!(
            path_of(&env),
            Some(r"C:\Program Files\PowerShell\7;C:\Windows\system32")
        );
        // 已经在 PATH 上就不再加
        let path = format!(r"{PS7};C:\Windows\system32");
        let env = plan(
            AgentKind::Codex,
            "pwsh",
            &path,
            &with_pwsh(&[PS7]),
            Some(PS7.into()),
            None,
        )
        .unwrap();
        assert!(env.is_empty());
    }

    #[test]
    fn git_bash_is_for_claude_only() {
        let bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
        let env = plan(
            AgentKind::ClaudeCode,
            "git_bash",
            "",
            &with_pwsh(&[]),
            None,
            Some(bash.clone()),
        )
        .unwrap();
        assert_eq!(
            env,
            vec![(
                "CLAUDE_CODE_GIT_BASH_PATH".into(),
                bash.display().to_string()
            )]
        );
        assert!(plan(
            AgentKind::ClaudeCode,
            "git_bash",
            "",
            &with_pwsh(&[]),
            None,
            None
        )
        .is_err());
        // Codex 只能用 PowerShell，选 Git Bash 等于自动
        assert!(plan(
            AgentKind::Codex,
            "git_bash",
            "",
            &with_pwsh(&[]),
            None,
            None
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn other_agents_and_bad_values() {
        let path = format!(r"C:\Windows\system32;{STORE}");
        assert!(plan(
            AgentKind::Gemini,
            "auto",
            &path,
            &with_pwsh(&[STORE]),
            None,
            None
        )
        .unwrap()
        .is_empty());
        assert!(plan(AgentKind::Codex, "cmd", "", &with_pwsh(&[]), None, None).is_err());
    }
}
