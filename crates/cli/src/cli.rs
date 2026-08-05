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

/// The single-instance protocol shared by the Zed app and the CLI on macOS.
///
/// The first instance of a release channel binds a TCP listener on a
/// per-channel, per-user localhost port and answers every connection with a
/// handshake string. Later instances (and the CLI) learn that an instance is
/// already running by connecting and reading that handshake back. The app side
/// of the protocol lives in the `zed` crate (`mac_only_instance`); the shared
/// pieces live here so the CLI cannot drift from the app.
#[cfg(target_os = "macos")]
pub mod mac_single_instance {
    use std::{
        io::Read,
        net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
        time::Duration,
    };

    use release_channel::ReleaseChannel;

    const LOCALHOST: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
    const CONNECT_TIMEOUT: Duration = Duration::from_millis(10);
    const RECEIVE_TIMEOUT: Duration = Duration::from_millis(35);
    /// Timeout for writing the handshake on the listening side.
    pub const SEND_TIMEOUT: Duration = Duration::from_millis(20);
    const USER_BLOCK: u16 = 100;

    pub fn address() -> SocketAddr {
        // These port numbers are offset by the user ID to avoid conflicts between
        // different users on the same machine. In addition to that the ports for each
        // release channel are spaced out by 100 to avoid conflicts between different
        // users running different release channels on the same machine. This ends up
        // interleaving the ports between different users and different release channels.
        //
        // On macOS user IDs start at 501 and on Linux they start at 1000. The first user
        // on a Mac with ID 501 running a dev channel build will use port 44238, and the
        // second user with ID 502 will use port 44239, and so on. User 501 will use ports
        // 44338, 44438, and 44538 for the preview, stable, and nightly channels,
        // respectively. User 502 will use ports 44339, 44439, and 44539 for the preview,
        // stable, and nightly channels, respectively.
        let port = match *release_channel::RELEASE_CHANNEL {
            ReleaseChannel::Dev => 43737,
            ReleaseChannel::Preview => 43737 + USER_BLOCK,
            ReleaseChannel::Stable => 43737 + (2 * USER_BLOCK),
            ReleaseChannel::Nightly => 43737 + (3 * USER_BLOCK),
        };
        let mut user_port = port;
        let uid = unsafe { libc::geteuid() };
        // Ensure that the user ID is not too large to avoid overflow when
        // calculating the port number. This seems unlikely but it doesn't
        // hurt to be safe.
        let max_port = 65535;
        let max_uid: u32 = max_port - port as u32;
        let wrapped_uid: u16 = (uid % max_uid) as u16;
        user_port += wrapped_uid;

        SocketAddr::V4(SocketAddrV4::new(LOCALHOST, user_port))
    }

    pub fn instance_handshake() -> &'static str {
        match *release_channel::RELEASE_CHANNEL {
            ReleaseChannel::Dev => "Zed Editor Dev Instance Running",
            ReleaseChannel::Nightly => "Zed Editor Nightly Instance Running",
            ReleaseChannel::Preview => "Zed Editor Preview Instance Running",
            ReleaseChannel::Stable => "Zed Editor Stable Instance Running",
        }
    }

    /// Whether an instance of this release channel is already running for this
    /// user, determined by reading the handshake back from the single-instance
    /// port.
    pub fn check_got_handshake() -> bool {
        match TcpStream::connect_timeout(&address(), CONNECT_TIMEOUT) {
            Ok(mut stream) => {
                let mut buf = vec![0u8; instance_handshake().len()];

                if let Err(err) = stream.set_read_timeout(Some(RECEIVE_TIMEOUT)) {
                    log::warn!("Failed to set single instance read timeout: {err}");
                    return false;
                }
                if let Err(err) = stream.read_exact(&mut buf) {
                    log::warn!("Connected to single instance port but failed to read: {err}");
                    return false;
                }

                if buf == instance_handshake().as_bytes() {
                    log::info!("Got instance handshake");
                    return true;
                }

                log::warn!("Got wrong instance handshake value");
                false
            }

            Err(_) => false,
        }
    }
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
