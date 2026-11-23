//! Windows 上从开始菜单启动的 GUI 程序继承的是登录时 Explorer 的环境：之后才装的 Agent
//! （`npm i -g`、winget、官方安装脚本）改了注册表里的 PATH，观澜却看不到，检测显示「未安装」。

#[cfg(windows)]
const SYSTEM_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// 从注册表重新读取系统和用户 PATH，把当前进程 PATH 里没有的目录补到末尾；其它平台不做事。
pub fn refresh_path() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        // 安装程序常把目录写成 `%NVM_SYMLINK%` 这类引用，变量本身也是登录后才加的：依次查进程环境、用户、系统。
        let lookup = |name: &str| {
            std::env::var(name)
                .ok()
                .or_else(|| win::read(HKEY_CURRENT_USER, "Environment", name, false))
                .or_else(|| win::read(HKEY_LOCAL_MACHINE, SYSTEM_KEY, name, false))
        };
        let extra = [
            win::read(HKEY_LOCAL_MACHINE, SYSTEM_KEY, "Path", true),
            win::read(HKEY_CURRENT_USER, "Environment", "Path", true),
        ]
        .map(|v| v.map(|s| expand(&s, &lookup)));
        let current = std::env::var_os("PATH").unwrap_or_default();
        if let Some(m) = merge(&current.to_string_lossy(), &extra) {
            std::env::set_var("PATH", m);
        }
    }
}

/// 展开 `%NAME%`；查不到的引用原样保留。
#[cfg_attr(not(windows), allow(dead_code))]
fn expand(s: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) if j > 0 => {
                let name = &after[..j];
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 把 `extra` 里不在 `current` 中的目录（不区分大小写、忽略末尾斜杠）依次追加；没有新目录时返回 None。
#[cfg_attr(not(windows), allow(dead_code))]
fn merge(current: &str, extra: &[Option<String>]) -> Option<String> {
    let key = |p: &str| p.trim().trim_end_matches(['/', '\\']).to_lowercase();
    let mut seen: Vec<String> = current
        .split(';')
        .map(key)
        .filter(|k| !k.is_empty())
        .collect();
    let mut out = current.trim_end_matches(';').to_string();
    let mut changed = false;
    for dir in extra.iter().flatten().flat_map(|s| s.split(';')) {
        let k = key(dir);
        if k.is_empty() || seen.contains(&k) {
            continue;
        }
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(dir.trim());
        seen.push(k);
        changed = true;
    }
    changed.then_some(out)
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// 读字符串值。`raw` 时保留 `%VAR%` 原样，否则 REG_EXPAND_SZ 由系统按当前进程环境展开。
    pub fn read(root: HKEY, sub: &str, name: &str, raw: bool) -> Option<String> {
        let sub = wide(sub);
        let name = wide(name);
        let flags = if raw {
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND
        } else {
            RRF_RT_REG_SZ
        };
        let mut len: u32 = 0;
        // SAFETY: 指针都指向活着的以 0 结尾的 UTF-16 缓冲区，长度按字节传入。
        unsafe {
            let first = RegGetValueW(
                root,
                sub.as_ptr(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut len,
            );
            if first != 0 || len == 0 {
                return None;
            }
            let mut buf = vec![0u16; (len as usize).div_ceil(2)];
            let r = RegGetValueW(
                root,
                sub.as_ptr(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut len,
            );
            if r != 0 {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{expand, merge};

    #[test]
    fn expands_known_vars_and_keeps_unknown() {
        let lookup = |n: &str| (n == "NVM_SYMLINK").then(|| "C:/nvm4w/nodejs".to_string());
        assert_eq!(
            expand(r"%NVM_SYMLINK%;%NOPE%;100%;a%%b", &lookup),
            "C:/nvm4w/nodejs;%NOPE%;100%;a%%b"
        );
    }

    #[test]
    fn appends_only_missing_dirs() {
        let got = merge(
            r"C:\Windows;C:\Tools\",
            &[
                Some(r"c:\windows;C:\Users\a\AppData\Roaming\npm".into()),
                None,
                Some(r"C:\tools;;".into()),
            ],
        );
        assert_eq!(
            got.as_deref(),
            Some(r"C:\Windows;C:\Tools\;C:\Users\a\AppData\Roaming\npm")
        );
        assert_eq!(merge(r"C:\Windows", &[Some(r"C:\Windows\".into())]), None);
    }
}
