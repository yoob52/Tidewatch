//! Windows 上从开始菜单启动的 GUI 程序继承的是登录时 Explorer 的环境：之后才装的 Agent
//! （`npm i -g`、winget、官方安装脚本）改了注册表里的 PATH，观澜却看不到，检测显示「未安装」。

/// 从注册表重新读取系统和用户 PATH，把当前进程 PATH 里没有的目录补到末尾；其它平台不做事。
pub fn refresh_path() {
    #[cfg(windows)]
    {
        let current = std::env::var_os("PATH").unwrap_or_default();
        let merged = merge(
            &current.to_string_lossy(),
            &[
                win::read(
                    windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
                    r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
                ),
                win::read(
                    windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
                    "Environment",
                ),
            ],
        );
        if let Some(m) = merged {
            std::env::set_var("PATH", m);
        }
    }
}

/// 把 `extra` 里不在 `current` 中的目录（不区分大小写、忽略末尾斜杠）依次追加；没有新目录时返回 None。
#[cfg_attr(not(windows), allow(dead_code))]
fn merge(current: &str, extra: &[Option<String>]) -> Option<String> {
    let key = |p: &str| {
        p.trim()
            .trim_end_matches(['/', std::path::MAIN_SEPARATOR])
            .to_lowercase()
    };
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
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY, RRF_RT_REG_SZ};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// 读 `Path` 值；REG_EXPAND_SZ 由系统按当前进程环境展开。
    pub fn read(root: HKEY, sub: &str) -> Option<String> {
        let sub = wide(sub);
        let name = wide("Path");
        let mut len: u32 = 0;
        // SAFETY: 指针都指向活着的以 0 结尾的 UTF-16 缓冲区，长度按字节传入。
        unsafe {
            let first = RegGetValueW(
                root,
                sub.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
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
                RRF_RT_REG_SZ,
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
    use super::merge;

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
