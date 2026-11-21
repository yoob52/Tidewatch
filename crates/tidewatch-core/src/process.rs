//! 进程树：Agent CLI 会派生子进程（shell、node、测试命令），停止时必须整棵树一起结束。
//! Windows 用 Job Object（KILL_ON_JOB_CLOSE），Unix 用进程组。
use crate::Result;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub struct ProcessTree {
    child: Child,
    #[cfg(windows)]
    job: win::Job,
}

/// 以独立进程树启动；stdin/stdout/stderr 均为管道。
pub fn spawn(mut cmd: Command) -> Result<ProcessTree> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(windows)]
    let job = win::Job::new()?;
    let child = cmd.spawn()?;
    #[cfg(windows)]
    job.assign(&child)?;
    Ok(ProcessTree {
        child,
        #[cfg(windows)]
        job,
    })
}

impl ProcessTree {
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        Ok(self.child.try_wait()?)
    }
    pub fn wait(&mut self) -> Result<ExitStatus> {
        Ok(self.child.wait()?)
    }

    /// 结束整棵进程树。Unix 先 SIGTERM，`grace` 内未退出再 SIGKILL。
    pub fn kill_tree(&mut self, grace: Duration) -> Result<()> {
        #[cfg(unix)]
        {
            let pgid = self.child.id() as libc::pid_t;
            unsafe { libc::killpg(pgid, libc::SIGTERM) };
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                if self.child.try_wait()?.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
        #[cfg(windows)]
        {
            let _ = (grace, Instant::now());
            self.job.terminate()?;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.kill_tree(Duration::from_millis(200));
            return;
        }
        // 主进程已自行退出：清掉它留在进程组里的后台后代。
        // Windows 上由 Job 的 KILL_ON_JOB_CLOSE 在句柄关闭时完成同样的事。
        #[cfg(unix)]
        unsafe {
            libc::killpg(self.child.id() as libc::pid_t, libc::SIGKILL);
        }
    }
}

/// 指定 pid 的进程是否还活着（用于孤儿进程检查与测试）。
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // 僵尸进程也算已结束
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            return !stat
                .rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z');
        }
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(windows)]
    {
        win::pid_alive(pid)
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    pub struct Job(HANDLE);
    unsafe impl Send for Job {}

    impl Job {
        pub fn new() -> io::Result<Self> {
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let job = Job(h);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(job)
            }
        }
        pub fn assign(&self, child: &Child) -> io::Result<()> {
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        pub fn terminate(&self) -> io::Result<()> {
            if unsafe { TerminateJobObject(self.0, 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn pid_alive(pid: u32) -> bool {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            ok != 0 && code == STILL_ACTIVE as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    /// 启动「父进程 + 孙进程」，父进程打印孙进程 pid 后挂起。
    fn parent_with_grandchild() -> Command {
        #[cfg(unix)]
        {
            let mut c = Command::new("sh");
            c.args(["-c", "sleep 300 & echo $!; wait"]);
            c
        }
        #[cfg(windows)]
        {
            let mut c = Command::new("powershell");
            c.args([
                "-NoProfile",
                "-Command",
                "$p = Start-Process ping -ArgumentList '-n','300','127.0.0.1' -PassThru -WindowStyle Hidden; Write-Output $p.Id; Start-Sleep 300",
            ]);
            c
        }
    }

    #[test]
    fn kill_tree_stops_grandchildren() {
        let mut tree = spawn(parent_with_grandchild()).unwrap();
        let mut line = String::new();
        BufReader::new(tree.take_stdout().unwrap())
            .read_line(&mut line)
            .unwrap();
        let grandchild: u32 = line.trim().parse().expect("grandchild pid");
        assert!(pid_alive(grandchild));
        assert!(tree.try_wait().unwrap().is_none());

        tree.kill_tree(Duration::from_secs(2)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while pid_alive(grandchild) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!pid_alive(grandchild), "孙进程应随进程树一起结束");
        assert!(!pid_alive(tree.id()));
    }

    #[test]
    fn drop_kills_running_tree() {
        let mut tree = spawn(parent_with_grandchild()).unwrap();
        let mut line = String::new();
        BufReader::new(tree.take_stdout().unwrap())
            .read_line(&mut line)
            .unwrap();
        let grandchild: u32 = line.trim().parse().unwrap();
        drop(tree);
        let deadline = Instant::now() + Duration::from_secs(5);
        while pid_alive(grandchild) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!pid_alive(grandchild));
    }
}
