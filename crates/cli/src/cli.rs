use std::path::PathBuf;

use anyhow::Result;
use collections::HashMap;
pub use ipc_channel::ipc;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct IpcHandshake {
    pub requests: ipc::IpcSender<CliRequest>,
    pub responses: ipc::IpcReceiver<CliResponse>,
}

/// Controls how CLI paths are opened — whether to reuse existing windows,
/// create new ones, or add to the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenBehavior {
    /// Consult the user's `cli_default_open_behavior` setting.
    #[default]
    Default,
    /// Always create a new window. No matching against existing worktrees.
    /// Corresponds to `zed -n`.
    AlwaysNew,
    /// Create a new window unless opening a subpath of an existing project.
    PreferNewWindow,
    /// Match broadly including subdirectories, and fall back to any existing
    /// window if no worktree matched. Corresponds to `zed -a`.
    Add,
    /// Open directories as a new workspace in the current Zed window's sidebar.
    /// Reuse existing windows for files in open worktrees.
    /// Corresponds to `zed -e`.
    ExistingWindow,
    /// New window for directories, reuse existing window for files in open
    /// worktrees. The classic pre-sidebar behavior.
    /// Corresponds to `zed --classic`.
    Classic,
    /// Replace the content of an existing window with a new workspace.
    /// Corresponds to `zed -r`.
    Reuse,
}

/// The setting-level enum for configuring default behavior. This only has
/// two values because the other modes are always explicitly requested via
/// CLI flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliBehaviorSetting {
    /// Open directories as a new workspace in the current Zed window's sidebar.
    ExistingWindow,
    /// Open paths in a new window unless they are subpaths of an existing project.
    NewWindow,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CliRequest {
    Open {
        paths: Vec<String>,
        urls: Vec<String>,
        diff_paths: Vec<[String; 2]>,
        diff_all: bool,
        wsl: Option<String>,
        wait: bool,
        #[serde(default)]
        open_behavior: OpenBehavior,
        env: Option<HashMap<String, String>>,
        user_data_dir: Option<String>,
        dev_container: bool,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    SetOpenBehavior {
        behavior: CliBehaviorSetting,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CliResponse {
    Ping,
    Stdout { message: String },
    Stderr { message: String },
    Exit { status: i32 },
    PromptOpenBehavior,
}

/// When Zed started not as an *.app but as a binary (e.g. local development),
/// there's a possibility to tell it to behave "regularly".
///
/// Note that in the main zed binary, this variable is unset after it's read for the first time,
/// therefore it should always be accessed through the `FORCE_CLI_MODE` static.
pub const FORCE_CLI_MODE_ENV_VAR_NAME: &str = "ZED_FORCE_CLI_MODE";

/// A running Zed instance sets this variable for its child processes (e.g.
/// built-in terminals). It holds the instance's private CLI endpoint — a
/// datagram socket path on Unix, a named pipe path on Windows — accepting
/// `zed-cli://` urls. When present, the CLI sends its request there so that
/// it reaches the exact instance it was spawned from, which channel-wide
/// instance discovery cannot guarantee when multiple instances are running.
pub const INSTANCE_SOCKET_ENV_VAR_NAME: &str = "ZED_CLI_SOCKET";

/// File name prefix of per-instance CLI sockets of this release channel (Unix).
/// The channel is embedded so that a CLI from one channel never sends requests
/// to an instance of another, whose IPC protocol may be incompatible.
#[cfg(unix)]
pub fn instance_socket_file_name_prefix() -> String {
    format!("zed-{}-", *release_channel::RELEASE_CHANNEL_NAME)
}

/// File name of the per-instance CLI socket of the Zed instance with the given
/// pid (Unix).
#[cfg(unix)]
pub fn instance_socket_file_name(pid: u32) -> String {
    format!("{}{pid}.sock", instance_socket_file_name_prefix())
}

/// Name prefix of per-instance CLI named pipes of this release channel
/// (Windows).
#[cfg(windows)]
pub fn instance_pipe_name_prefix() -> String {
    format!(
        "\\\\.\\pipe\\{}-Instance-Pipe-",
        release_channel::app_identifier()
    )
}

/// Name of the per-instance CLI named pipe of the Zed instance with the given
/// pid (Windows).
#[cfg(windows)]
pub fn instance_pipe_name(pid: u32) -> String {
    format!("{}{pid}", instance_pipe_name_prefix())
}

/// Abstracts the transport for sending CLI responses (Zed → CLI).
///
/// Production code uses `IpcSender<CliResponse>`. Tests can provide in-memory
/// implementations to avoid OS-level IPC.
pub trait CliResponseSink: Send + 'static {
    fn send(&self, response: CliResponse) -> Result<()>;
}

impl CliResponseSink for ipc::IpcSender<CliResponse> {
    fn send(&self, response: CliResponse) -> Result<()> {
        ipc::IpcSender::send(self, response).map_err(|error| anyhow::anyhow!("{error}"))
    }
}
