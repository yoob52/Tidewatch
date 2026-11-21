//! 密钥存储：数据库只存条目名（`secret_ref`），密钥本身放系统凭据管理器。
//!
//! Windows 用凭据管理器（观澜的目标平台）。其它系统没有统一的免依赖方案，
//! 开发时退回到数据目录下只有本人可读的文件。
use crate::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, secret: &str) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
    /// 给用户看的存放位置说明。
    fn describe(&self) -> String;
}

/// 当前系统的默认存储。
pub fn default_store(data_dir: &std::path::Path) -> Box<dyn SecretStore> {
    #[cfg(windows)]
    {
        let _ = data_dir;
        Box::new(windows::CredentialManager)
    }
    #[cfg(not(windows))]
    {
        Box::new(FileStore::new(data_dir.join("secrets.json")))
    }
}

/// 测试用：只在内存里。
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        self.0.lock().unwrap().insert(name.into(), secret.into());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<()> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
    fn describe(&self) -> String {
        "内存（仅测试）".into()
    }
}

/// 非 Windows 的退路：一个 JSON 文件，权限 0600。
pub struct FileStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        FileStore {
            path,
            lock: Mutex::new(()),
        }
    }
    fn load(&self) -> Result<HashMap<String, String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(serde_json::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(e) => Err(e.into()),
        }
    }
    fn store(&self, map: &HashMap<String, String>) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        // 先写临时文件再改名：写到一半失败或退出，原来的文件保持完整
        let tmp = self.path.with_extension("tmp");
        let mut f = opts.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        std::io::Write::write_all(&mut f, serde_json::to_string(map)?.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

impl SecretStore for FileStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        let _g = self.lock.lock().unwrap();
        Ok(self.load()?.remove(name))
    }
    fn set(&self, name: &str, secret: &str) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut map = self.load()?;
        map.insert(name.into(), secret.into());
        self.store(&map)
    }
    fn delete(&self, name: &str) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut map = self.load()?;
        if map.remove(name).is_some() {
            self.store(&map)?;
        }
        Ok(())
    }
    fn describe(&self) -> String {
        format!("本地文件 {}（仅本人可读）", self.path.display())
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;
    use crate::Error;
    use std::ptr;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    /// Windows 凭据管理器里的"普通凭据"，条目名即 `secret_ref`。
    pub struct CredentialManager;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error(what: &str) -> Error {
        Error::invalid(format!("凭据管理器{what}失败（错误码 {}）", unsafe {
            GetLastError()
        }))
    }

    impl SecretStore for CredentialManager {
        fn get(&self, name: &str) -> Result<Option<String>> {
            let target = wide(name);
            let mut cred: *mut CREDENTIALW = ptr::null_mut();
            // SAFETY: target 以 0 结尾；成功时 cred 指向系统分配的结构，用完 CredFree。
            unsafe {
                if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) == 0 {
                    return if GetLastError() == ERROR_NOT_FOUND {
                        Ok(None)
                    } else {
                        Err(last_error("读取"))
                    };
                }
                let c = &*cred;
                let bytes = if c.CredentialBlob.is_null() {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                };
                let secret = String::from_utf8_lossy(bytes).to_string();
                CredFree(cred as *const _);
                Ok(Some(secret))
            }
        }

        fn set(&self, name: &str, secret: &str) -> Result<()> {
            let mut target = wide(name);
            let mut user = wide("tidewatch");
            let mut blob = secret.as_bytes().to_vec();
            // SAFETY: 全零的 CREDENTIALW 是合法初值；各指针在调用期间有效。
            let ok = unsafe {
                let mut cred: CREDENTIALW = std::mem::zeroed();
                cred.Type = CRED_TYPE_GENERIC;
                cred.TargetName = target.as_mut_ptr();
                cred.CredentialBlobSize = blob.len() as u32;
                cred.CredentialBlob = blob.as_mut_ptr();
                cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
                cred.UserName = user.as_mut_ptr();
                CredWriteW(&cred, 0)
            };
            if ok == 0 {
                return Err(last_error("写入"));
            }
            Ok(())
        }

        fn delete(&self, name: &str) -> Result<()> {
            let target = wide(name);
            // SAFETY: target 以 0 结尾。
            unsafe {
                if CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) == 0
                    && GetLastError() != ERROR_NOT_FOUND
                {
                    return Err(last_error("删除"));
                }
            }
            Ok(())
        }

        fn describe(&self) -> String {
            "Windows 凭据管理器".into()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn credential_manager_roundtrip() {
            let name = format!("tidewatch-test/{}", crate::util::new_id());
            let store = CredentialManager;
            assert_eq!(store.get(&name).unwrap(), None);
            store.set(&name, "sk-测试-1234").unwrap();
            assert_eq!(store.get(&name).unwrap().as_deref(), Some("sk-测试-1234"));
            store.set(&name, "second").unwrap();
            assert_eq!(store.get(&name).unwrap().as_deref(), Some("second"));
            store.delete(&name).unwrap();
            assert_eq!(store.get(&name).unwrap(), None);
            store.delete(&name).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/secrets.json");
        let store = FileStore::new(path.clone());
        assert_eq!(store.get("p1").unwrap(), None);
        store.set("p1", "sk-one").unwrap();
        store.set("p2", "sk-two").unwrap();
        assert_eq!(store.get("p1").unwrap().as_deref(), Some("sk-one"));
        store.delete("p1").unwrap();
        assert_eq!(store.get("p1").unwrap(), None);
        assert_eq!(store.get("p2").unwrap().as_deref(), Some("sk-two"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
