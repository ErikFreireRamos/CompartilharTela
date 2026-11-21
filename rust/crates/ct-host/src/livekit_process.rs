//! Sobe e derruba o livekit-server.exe embutido, com chaves novas a cada sala.

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use ct_core::protocol::{LIVEKIT_INTERNAL_PORT, MAX_PARTICIPANTS, RTC_TCP_PORT, RTC_UDP_PORT};
use reqwest::Client;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub struct LiveKitPaths {
    pub exe: PathBuf,
    pub work_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum LiveKitError {
    #[error("livekit-server não encontrado em {0}")]
    BinaryNotFound(PathBuf),
    #[error("Erro de I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("O servidor LiveKit fechou inesperadamente (código {0:?})")]
    ExitedEarly(Option<i32>),
    #[error("O servidor LiveKit não respondeu a tempo")]
    StartupTimeout,
}

pub struct LiveKitProcess {
    paths: LiveKitPaths,
    pub api_key: String,
    pub api_secret: String,
    child: Option<Child>,
    #[cfg(windows)]
    job_handle: Option<JobHandle>,
}

#[cfg(windows)]
struct JobHandle(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for JobHandle {}
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

impl LiveKitProcess {
    pub fn new(paths: LiveKitPaths) -> Self {
        let key_bytes: [u8; 6] = rand::random();
        let secret_bytes: [u8; 32] = rand::random();
        let api_key = format!(
            "API{}",
            key_bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let api_secret = secret_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        Self {
            paths,
            api_key,
            api_secret,
            child: None,
            #[cfg(windows)]
            job_handle: None,
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{LIVEKIT_INTERNAL_PORT}")
    }

    pub fn render_config(&self, node_ip: Option<&str>) -> String {
        let mut lines = vec![
            format!("port: {LIVEKIT_INTERNAL_PORT}"),
            "logging:".to_string(),
            "  level: info".to_string(),
            "rtc:".to_string(),
            format!("  tcp_port: {RTC_TCP_PORT}"),
            format!("  udp_port: {RTC_UDP_PORT}"),
            "  use_external_ip: false".to_string(),
        ];
        if let Some(ip) = node_ip {
            lines.push(format!("  node_ip: {ip}"));
            lines.push("  advertise_internal_ip: true".to_string());
        }
        lines.extend([
            "  enable_loopback_candidate: true".to_string(),
            "keys:".to_string(),
            format!("  {}: {}", self.api_key, self.api_secret),
            "room:".to_string(),
            "  auto_create: false".to_string(),
            format!("  max_participants: {MAX_PARTICIPANTS}"),
            "  empty_timeout: 600".to_string(),
            "  departure_timeout: 20".to_string(),
            String::new(),
        ]);
        lines.join("\n")
    }

    pub async fn start(&mut self, node_ip: Option<&str>) -> Result<(), LiveKitError> {
        if !self.paths.exe.exists() {
            return Err(LiveKitError::BinaryNotFound(self.paths.exe.clone()));
        }
        fs::create_dir_all(&self.paths.work_dir)?;
        let config_path = self.paths.work_dir.join("livekit.yaml");
        fs::write(&config_path, self.render_config(node_ip))?;

        let log_path = self.paths.work_dir.join("livekit.log");
        let log_out = File::create(&log_path)?;
        let log_err = log_out.try_clone()?;

        let mut cmd = Command::new(&self.paths.exe);
        cmd.arg("--config")
            .arg(&config_path)
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err))
            .kill_on_drop(true);

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let child = cmd.spawn()?;
        #[cfg(windows)]
        if let Some(pid) = child.id() {
            self.job_handle = attach_kill_on_close_job(pid);
        }
        self.child = Some(child);

        if let Err(err) = self.wait_ready(Duration::from_secs(15)).await {
            self.stop();
            return Err(err);
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
        #[cfg(windows)]
        {
            self.job_handle.take();
        }
    }

    async fn wait_ready(&mut self, timeout_dur: Duration) -> Result<(), LiveKitError> {
        let client = Client::new();
        let deadline = Instant::now() + timeout_dur;
        while Instant::now() < deadline {
            if let Some(child) = self.child.as_mut()
                && let Ok(Some(status)) = child.try_wait()
            {
                return Err(LiveKitError::ExitedEarly(status.code()));
            }
            if let Ok(res) = client
                .get(self.url())
                .timeout(Duration::from_secs(1))
                .send()
                .await
                && res.status().is_success()
            {
                return Ok(());
            }
            sleep(Duration::from_millis(200)).await;
        }
        Err(LiveKitError::StartupTimeout)
    }
}

impl Drop for LiveKitProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
fn attach_kill_on_close_job(pid: u32) -> Option<JobHandle> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    unsafe {
        let job = CreateJobObjectW(None, None).ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let _ = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if let Ok(proc) = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
            let _ = AssignProcessToJobObject(job, proc);
            let _ = CloseHandle(proc);
        }
        Some(JobHandle(job))
    }
}
