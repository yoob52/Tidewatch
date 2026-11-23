//! Windows 上 Agent 跑命令用哪个 Shell（设置里的「Agent 使用的 Shell」）。
//!
//! - Codex 只用 PowerShell：PATH 上有 `pwsh.exe` 就用它，没有就用系统自带的 `powershell.exe`，
//!   不读 `SHELL` / `COMSPEC`。所以这里靠改这次运行的 PATH 来选。
//! - Claude Code 默认用 Git Bash（`CLAUDE_CODE_GIT_BASH_PATH` 指定），
//!   `CLAUDE_CODE_USE_POWERSHELL_TOOL=1` 再给它一个 PowerShell 工具。
//! - 两个都不能把 cmd 当 Shell。
//!
//! Microsoft Store 版 PowerShell 在 PATH 上只是 WindowsApps 里的别名，Agent 的沙箱启动它会报
//! 「拒绝访问」，所以跳过别名，需要时从注册的包找到真正的 pwsh.exe。
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
    plan(
        kind,
        choice,
        &path,
        &|p| p.is_file(),
        || {
            if choice == "pwsh" {
                let msi = std::env::var_os("ProgramFiles")
                    .map(|p| PathBuf::from(p).join("PowerShell").join("7"))
                    .filter(|d| d.join("pwsh.exe").is_file());
                if msi.is_some() {
                    return msi;
                }
            }
            store_pwsh_dir()
        },
        git_bash(&path),
    )
}

#[cfg(windows)]
fn store_pwsh_dir() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let powershell = PathBuf::from(std::env::var_os("SystemRoot")?)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = Command::new(powershell)
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding = [Text.Encoding]::UTF8; \
             (Appx\\Get-AppxPackage -Name Microsoft.PowerShell).InstallLocation",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let locations = String::from_utf8(output.stdout).ok()?;
    locations
        .lines()
        .map(|line| PathBuf::from(line.trim()))
        .filter(|dir| dir.is_absolute())
        .find(|dir| {
            let pwsh = dir.join("pwsh.exe");
            pwsh.is_file()
                && Command::new(pwsh)
                    .creation_flags(CREATE_NO_WINDOW)
                    .args(["-NoProfile", "-NonInteractive", "-Version"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success())
        })
}

#[cfg(not(windows))]
fn store_pwsh_dir() -> Option<PathBuf> {
    None
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
    let dir = dir
        .trim_matches('"')
        .trim_end_matches(['/', '\\'])
        .to_ascii_lowercase()
        .replace('/', "\\");
    dir.ends_with(r"\microsoft\windowsapps") || dir.contains(r"\microsoft\windowsapps\")
}

pub(crate) fn plan(
    kind: AgentKind,
    choice: &str,
    path: &str,
    exists: &dyn Fn(&Path) -> bool,
    real_pwsh_dir: impl FnOnce() -> Option<PathBuf>,
    git_bash: Option<PathBuf>,
) -> Result<Vec<(String, String)>> {
    validate(choice)?;
    if !matches!(kind, AgentKind::Codex | AgentKind::ClaudeCode) {
        return Ok(vec![]);
    }
    let has_pwsh = |d: &str| exists(&Path::new(d.trim_matches('"')).join("pwsh.exe"));
    let mut has_store_alias = false;
    let mut dirs: Vec<String> = path
        .split(';')
        .filter(|d| !d.is_empty())
        .filter(|d| {
            let is_alias = is_store_alias_dir(d) && has_pwsh(d);
            has_store_alias |= is_alias;
            !is_alias
        })
        .map(String::from)
        .collect();
    let mut env = vec![];
    match choice {
        "pwsh" => {
            if !dirs.iter().any(|d| has_pwsh(d)) {
                let Some(d) = real_pwsh_dir() else {
                    return Err(Error::invalid(
                        "没找到可启动的 PowerShell 7。请确认已安装且能正常打开 PowerShell 7，或改选 Windows PowerShell。",
                    ));
                };
                dirs.insert(0, d.display().to_string());
            }
        }
        "powershell" => dirs.retain(|d| !has_pwsh(d)),
        "auto" | "git_bash"
            if kind == AgentKind::Codex && has_store_alias && !dirs.iter().any(|d| has_pwsh(d)) =>
        {
            if let Some(d) = real_pwsh_dir() {
                dirs.insert(0, d.display().to_string());
            }
        }
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
    const STORE_PACKAGE: &str =
        r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.7.0.0_x64__8wekyb3d8bbwe";
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
            || None,
            None,
        )
        .unwrap();
        assert_eq!(path_of(&env), Some(r"C:\Windows\system32"));
        // 别名目录里没有 pwsh 就不动 PATH
        let env = plan(
            AgentKind::Codex,
            "auto",
            &path,
            &with_pwsh(&[]),
            || None,
            None,
        )
        .unwrap();
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
            || None,
            None,
        )
        .unwrap();
        assert_eq!(path_of(&env), Some(r"C:\Windows\system32"));
        let env = plan(
            AgentKind::ClaudeCode,
            "powershell",
            &path,
            &with_pwsh(&[PS7]),
            || None,
            None,
        )
        .unwrap();
        assert!(env.contains(&("CLAUDE_CODE_USE_POWERSHELL_TOOL".into(), "1".into())));
    }

    #[test]
    fn pwsh_needs_a_real_install() {
        let path = format!(r"C:\Windows\system32;{STORE}");
        let only_store = with_pwsh(&[STORE]);
        assert!(plan(AgentKind::Codex, "pwsh", &path, &only_store, || None, None).is_err());
        let env = plan(
            AgentKind::Codex,
            "pwsh",
            &path,
            &only_store,
            || Some(PS7.into()),
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
            || Some(PS7.into()),
            None,
        )
        .unwrap();
        assert!(env.is_empty());
    }

    #[test]
    fn store_package_replaces_both_forms_of_pwsh_alias() {
        let family_alias = format!(r"{STORE}\Microsoft.PowerShell_8wekyb3d8bbwe");
        let path = format!(r"{STORE};C:\Windows\system32;{family_alias}");
        for (kind, choice) in [
            (AgentKind::Codex, "pwsh"),
            (AgentKind::Codex, "auto"),
            (AgentKind::Codex, "git_bash"),
            (AgentKind::ClaudeCode, "pwsh"),
        ] {
            let env = plan(
                kind,
                choice,
                &path,
                &with_pwsh(&[STORE, &family_alias]),
                || Some(STORE_PACKAGE.into()),
                None,
            )
            .unwrap();
            let expected = format!(r"{STORE_PACKAGE};C:\Windows\system32");
            assert_eq!(path_of(&env), Some(expected.as_str()));
            if kind == AgentKind::ClaudeCode {
                assert!(env.contains(&("CLAUDE_CODE_USE_POWERSHELL_TOOL".into(), "1".into())));
            }
        }
    }

    #[test]
    fn real_pwsh_on_path_does_not_need_package_lookup() {
        let path = format!(r"{STORE};{STORE_PACKAGE};C:\Windows\system32");
        let env = plan(
            AgentKind::Codex,
            "pwsh",
            &path,
            &with_pwsh(&[STORE, STORE_PACKAGE]),
            || panic!("pwsh already on PATH"),
            None,
        )
        .unwrap();
        let expected = format!(r"{STORE_PACKAGE};C:\Windows\system32");
        assert_eq!(path_of(&env), Some(expected.as_str()));
    }

    #[test]
    fn git_bash_is_for_claude_only() {
        let bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
        let env = plan(
            AgentKind::ClaudeCode,
            "git_bash",
            "",
            &with_pwsh(&[]),
            || None,
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
            || None,
            None
        )
        .is_err());
        // Codex 只能用 PowerShell，选 Git Bash 等于自动
        assert!(plan(
            AgentKind::Codex,
            "git_bash",
            "",
            &with_pwsh(&[]),
            || None,
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
            || None,
            None
        )
        .unwrap()
        .is_empty());
        assert!(plan(AgentKind::Codex, "cmd", "", &with_pwsh(&[]), || None, None).is_err());
    }
}
