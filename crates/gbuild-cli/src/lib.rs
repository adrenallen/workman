//! Command-line client for the authenticated gbuild daemon control channel.

use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, io,
    os::fd::RawFd,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use gbuild_core::{Process, ProcessStatus, ProjectId};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const DAEMON_WAIT: Duration = Duration::from_secs(5);
const OUTPUT_POLL: Duration = Duration::from_millis(30);
const MAX_OUTPUT_CHUNK: usize = 64 * 1024;

const HELP: &str = "\
Usage: gbuild [--data-dir PATH] [--daemon PATH] <COMMAND>\n\
\n\
Commands:\n\
  run [--project ID] [--name NAME] [--cwd PATH] -- <command...>\n\
      Create and start a durable command process. GBUILD_PROJECT_ID is used when\n\
      --project is absent.\n\
  ps [--project ID]\n\
      List process IDs, project IDs, statuses, PIDs, names, and commands.\n\
  logs [-f|--follow] <PROCESS_ID>\n\
      Print daemon-rendered output, or follow the live raw stream.\n\
  attach <PROCESS_ID>\n\
      Replay and attach to the live PTY with raw input and resize forwarding.\n\
  stop <PROCESS_ID>\n\
      Gracefully stop the process group.\n";

/// Parse process arguments and execute one CLI invocation.
pub async fn run_env() -> Result<()> {
    let cli = Cli::parse(env::args_os())?;
    if matches!(cli.command, Command::Help) {
        print!("{HELP}");
        return Ok(());
    }

    let data_dir = cli.data_dir.unwrap_or_else(gbuildd::default_data_dir);
    let daemon = daemon_executable(cli.daemon);
    let mut client = Client::connect(&data_dir, &daemon).await?;

    match cli.command {
        Command::Run(options) => run_command(&mut client, options).await,
        Command::Ps { project_id } => ps(&mut client, project_id).await,
        Command::Logs { process_id, follow } => logs(&mut client, process_id, follow).await,
        Command::Attach { process_id } => attach(&mut client, process_id).await,
        Command::Stop { process_id } => stop(&mut client, process_id).await,
        Command::Help => unreachable!(),
    }
}

#[derive(Debug)]
struct Cli {
    data_dir: Option<PathBuf>,
    daemon: Option<PathBuf>,
    command: Command,
}

#[derive(Debug)]
enum Command {
    Run(RunOptions),
    Ps { project_id: Option<ProjectId> },
    Logs { process_id: i64, follow: bool },
    Attach { process_id: i64 },
    Stop { process_id: i64 },
    Help,
}

#[derive(Debug)]
struct RunOptions {
    project_id: Option<ProjectId>,
    name: Option<String>,
    cwd: Option<PathBuf>,
    command: Vec<String>,
}

impl Cli {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter();
        let _program = args.next();
        let mut args = args
            .map(|arg| {
                arg.into_string()
                    .map_err(|_| cli_error("arguments must be valid UTF-8"))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter();

        let mut data_dir = None;
        let mut daemon = None;
        let command = loop {
            let Some(arg) = args.next() else {
                return Err(cli_error(format!("a command is required\n\n{HELP}")));
            };
            match arg.as_str() {
                "--data-dir" => data_dir = Some(PathBuf::from(next_value(&mut args, &arg)?)),
                "--daemon" => daemon = Some(PathBuf::from(next_value(&mut args, &arg)?)),
                "--help" | "-h" | "help" => break Command::Help,
                "run" => break parse_run(args)?,
                "ps" => break parse_ps(args)?,
                "logs" => break parse_logs(args)?,
                "attach" => {
                    break Command::Attach {
                        process_id: parse_single_process_id(args, "attach")?,
                    };
                }
                "stop" => {
                    break Command::Stop {
                        process_id: parse_single_process_id(args, "stop")?,
                    };
                }
                _ => return Err(cli_error(format!("unknown command {arg:?}\n\n{HELP}"))),
            }
        };

        Ok(Self {
            data_dir,
            daemon,
            command,
        })
    }
}

fn parse_run(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let mut project_id = None;
    let mut name = None;
    let mut cwd = None;
    let mut command = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                command.extend(args);
                break;
            }
            "--project" if command.is_empty() => {
                project_id = Some(parse_id(&next_value(&mut args, &arg)?, "project")?);
            }
            "--name" if command.is_empty() => name = Some(next_value(&mut args, &arg)?),
            "--cwd" if command.is_empty() => {
                cwd = Some(PathBuf::from(next_value(&mut args, &arg)?));
            }
            _ => {
                command.push(arg);
                command.extend(args);
                break;
            }
        }
    }
    if command.is_empty() {
        return Err(cli_error("run requires a command"));
    }
    Ok(Command::Run(RunOptions {
        project_id,
        name,
        cwd,
        command,
    }))
}

fn parse_ps(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let mut project_id = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project_id = Some(parse_id(&next_value(&mut args, &arg)?, "project")?),
            _ => return Err(cli_error(format!("unknown ps option {arg:?}"))),
        }
    }
    Ok(Command::Ps { project_id })
}

fn parse_logs(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let mut follow = false;
    let mut process_id = None;
    for arg in args.by_ref() {
        match arg.as_str() {
            "--follow" | "-f" => follow = true,
            _ if process_id.is_none() => process_id = Some(parse_id(&arg, "process")?),
            _ => return Err(cli_error("logs accepts exactly one process ID")),
        }
    }
    Ok(Command::Logs {
        process_id: process_id.ok_or_else(|| cli_error("logs requires a process ID"))?,
        follow,
    })
}

fn parse_single_process_id(mut args: impl Iterator<Item = String>, command: &str) -> Result<i64> {
    let id = args
        .next()
        .ok_or_else(|| cli_error(format!("{command} requires a process ID")))?;
    if args.next().is_some() {
        return Err(cli_error(format!(
            "{command} accepts exactly one process ID"
        )));
    }
    parse_id(&id, "process")
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String> {
    args.next()
        .ok_or_else(|| cli_error(format!("{option} requires a value")))
}

fn parse_id(value: &str, kind: &str) -> Result<i64> {
    let id = value
        .parse::<i64>()
        .map_err(|_| cli_error(format!("{kind} ID must be a positive integer")))?;
    if id <= 0 {
        return Err(cli_error(format!("{kind} ID must be a positive integer")));
    }
    Ok(id)
}

async fn run_command(client: &mut Client, options: RunOptions) -> Result<()> {
    let cwd = options
        .cwd
        .unwrap_or(env::current_dir()?)
        .canonicalize()
        .map_err(|error| cli_error(format!("invalid working directory: {error}")))?;
    let project_id = resolve_project(options.project_id)?;
    let command = shell_command(&options.command);
    let name = options.name.unwrap_or_else(|| {
        Path::new(&options.command[0])
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&options.command[0])
            .to_owned()
    });

    let created: Process = client
        .rpc(
            "process.create",
            json!({
                "id": 0,
                "project_id": project_id,
                "kind": "command",
                "name": name,
                "command": command,
                "working_dir": cwd,
                "env": {},
                "auto_start": false,
                "auto_restart": false,
                "restart_when_changed": [],
                "source": "local",
                "trust_hash": null,
                "status": "stopped",
                "pid": null,
                "exit_code": null,
                "exit_signal": null,
                "exited_at": null,
                "agent_tool_id": null
            }),
        )
        .await?;
    let started: Process = client
        .rpc("process.start", json!({ "process_id": created.id }))
        .await?;
    println!("Started process {} ({})", started.id, started.name);
    Ok(())
}

fn resolve_project(explicit: Option<ProjectId>) -> Result<ProjectId> {
    if let Some(id) = explicit {
        return Ok(id);
    }
    if let Ok(value) = env::var("GBUILD_PROJECT_ID") {
        return parse_id(&value, "project");
    }

    Err(cli_error("run requires --project ID or GBUILD_PROJECT_ID"))
}

async fn ps(client: &mut Client, project_id: Option<ProjectId>) -> Result<()> {
    let processes: Vec<Process> = client
        .rpc("process.list", json!({ "project_id": project_id }))
        .await?;
    println!(
        "{:<6} {:<8} {:<10} {:<8} {:<20} COMMAND",
        "ID", "PROJECT", "STATUS", "PID", "NAME"
    );
    for process in processes {
        println!(
            "{:<6} {:<8} {:<10} {:<8} {:<20} {}",
            process.id,
            process.project_id,
            process.status,
            process
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".into()),
            process.name,
            process.command.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct RenderedOutput {
    text: String,
}

#[derive(Debug, Deserialize)]
struct RawChunk {
    data: String,
    start_offset: u64,
    end_offset: u64,
    total_bytes: u64,
    status: ProcessStatus,
}

impl RawChunk {
    fn bytes(&self) -> Result<Vec<u8>> {
        BASE64
            .decode(&self.data)
            .map_err(|error| cli_error(format!("daemon returned invalid output data: {error}")))
    }
}

async fn logs(client: &mut Client, process_id: i64, follow: bool) -> Result<()> {
    if !follow {
        let output: RenderedOutput = client
            .rpc(
                "process.rendered_output",
                json!({ "process_id": process_id }),
            )
            .await?;
        let text = output.text.trim_end();
        if !text.is_empty() {
            println!("{text}");
        }
        return Ok(());
    }

    let mut stdout = tokio::io::stdout();
    let (mut offset, mut status) = drain_raw(client, process_id, None, &mut stdout).await?;
    let mut poll = tokio::time::interval(OUTPUT_POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if !is_active(status) {
            break;
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = poll.tick() => {
                (offset, status) = drain_raw(client, process_id, Some(offset), &mut stdout).await?;
            }
        }
    }
    stdout.flush().await?;
    Ok(())
}

async fn attach(client: &mut Client, process_id: i64) -> Result<()> {
    let mut stdout = tokio::io::stdout();
    let (mut offset, mut status) = drain_raw(client, process_id, None, &mut stdout).await?;
    if !is_active(status) {
        stdout.flush().await?;
        return Ok(());
    }

    let _raw_mode = RawModeGuard::enable(libc::STDIN_FILENO)?;
    send_resize(client, process_id).await?;
    let mut resize = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())?;
    let mut stdin = tokio::io::stdin();
    let mut stdin_open = true;
    let mut input = [0_u8; 8192];
    let mut poll = tokio::time::interval(OUTPUT_POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            read = stdin.read(&mut input), if stdin_open => {
                let count = read?;
                if count == 0 {
                    stdin_open = false;
                } else {
                    let _: Process = client.rpc(
                        "process.send_input",
                        json!({
                            "process_id": process_id,
                            "data": BASE64.encode(&input[..count]),
                        }),
                    ).await?;
                }
            }
            _ = resize.recv() => send_resize(client, process_id).await?,
            _ = poll.tick() => {
                (offset, status) = drain_raw(client, process_id, Some(offset), &mut stdout).await?;
                if !is_active(status) {
                    break;
                }
            }
        }
    }
    stdout.flush().await?;
    Ok(())
}

async fn send_resize(client: &mut Client, process_id: i64) -> Result<()> {
    let size = terminal_size(libc::STDOUT_FILENO).unwrap_or(TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    });
    let _: Process = client
        .rpc(
            "process.resize",
            json!({
                "process_id": process_id,
                "rows": size.rows,
                "cols": size.cols,
                "pixel_width": size.pixel_width,
                "pixel_height": size.pixel_height,
            }),
        )
        .await?;
    Ok(())
}

async fn drain_raw(
    client: &mut Client,
    process_id: i64,
    mut offset: Option<u64>,
    stdout: &mut tokio::io::Stdout,
) -> Result<(u64, ProcessStatus)> {
    loop {
        let chunk: RawChunk = client
            .rpc(
                "process.raw_output",
                json!({
                    "process_id": process_id,
                    "offset": offset,
                    "max_bytes": MAX_OUTPUT_CHUNK,
                }),
            )
            .await?;
        if let Some(requested) = offset
            && chunk.start_offset > requested
        {
            eprintln!(
                "gbuild: output before byte {} is no longer retained",
                chunk.start_offset
            );
        }
        let bytes = chunk.bytes()?;
        if !bytes.is_empty() {
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
        if chunk.end_offset >= chunk.total_bytes {
            return Ok((chunk.end_offset, chunk.status));
        }
        offset = Some(chunk.end_offset);
    }
}

async fn stop(client: &mut Client, process_id: i64) -> Result<()> {
    let process: Process = client
        .rpc("process.stop", json!({ "process_id": process_id }))
        .await?;
    println!("Stopped process {} ({})", process.id, process.name);
    Ok(())
}

fn is_active(status: ProcessStatus) -> bool {
    matches!(status, ProcessStatus::Starting | ProcessStatus::Running)
}

fn shell_command(args: &[String]) -> String {
    if args.len() == 1 {
        return args[0].clone();
    }
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_+-.,/:=@%".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn daemon_executable(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(path) = env::var_os("GBUILD_DAEMON") {
        return PathBuf::from(path);
    }
    if let Ok(current) = env::current_exe() {
        let sibling = current.with_file_name("gbuildd");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("gbuildd")
}

struct Client {
    socket: Socket,
    next_id: u64,
}

impl Client {
    async fn connect(data_dir: &Path, daemon: &Path) -> Result<Self> {
        let discovery = gbuildd::discover_or_spawn(data_dir, daemon, DAEMON_WAIT).await?;
        let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {}", discovery.token).parse()?,
        );
        let (socket, _) = connect_async(request).await?;
        Ok(Self { socket, next_id: 1 })
    }

    async fn rpc<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> Result<T> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.socket
            .send(Message::Text(
                json!({ "id": id, "method": method, "params": params })
                    .to_string()
                    .into(),
            ))
            .await?;

        loop {
            let message = self
                .socket
                .next()
                .await
                .ok_or_else(|| cli_error("daemon closed the control connection"))??;
            match message {
                Message::Text(text) => {
                    let response: RpcResponse = serde_json::from_str(&text)?;
                    if response.id != id {
                        return Err(cli_error(format!(
                            "daemon response ID mismatch: expected {id}, got {}",
                            response.id
                        )));
                    }
                    if !response.ok {
                        let error = response.error.unwrap_or(RpcError {
                            code: "unknown_error".into(),
                            message: "daemon request failed".into(),
                        });
                        return Err(Box::new(error));
                    }
                    return Ok(serde_json::from_value(response.result)?);
                }
                Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                Message::Close(_) => {
                    return Err(cli_error("daemon closed the control connection"));
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    id: u64,
    ok: bool,
    #[serde(default)]
    result: Value,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: String,
    message: String,
}

impl fmt::Display for RpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for RpcError {}

fn cli_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::other(message.into()))
}

struct RawModeGuard {
    fd: RawFd,
    original: libc::termios,
    enabled: bool,
}

impl RawModeGuard {
    fn enable(fd: RawFd) -> io::Result<Self> {
        // SAFETY: isatty only inspects the valid process-owned file descriptor.
        if unsafe { libc::isatty(fd) } != 1 {
            return Ok(Self {
                fd,
                // SAFETY: a zeroed termios is never used when raw mode is disabled.
                original: unsafe { std::mem::zeroed() },
                enabled: false,
            });
        }

        // SAFETY: tcgetattr initializes the termios value for a valid terminal fd.
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        // SAFETY: pointers are valid and the descriptor passed is a terminal.
        if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut raw = original;
        // SAFETY: cfmakeraw mutates an initialized termios structure in place.
        unsafe { libc::cfmakeraw(&mut raw) };
        // SAFETY: pointers are valid and the descriptor passed is a terminal.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            fd,
            original,
            enabled: true,
        })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.enabled {
            // SAFETY: original came from tcgetattr for this same terminal descriptor.
            let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
        }
    }
}

#[derive(Clone, Copy)]
struct TerminalSize {
    rows: u16,
    cols: u16,
    pixel_width: u16,
    pixel_height: u16,
}

fn terminal_size(fd: RawFd) -> io::Result<TerminalSize> {
    // SAFETY: winsize is a plain integer struct initialized before ioctl writes it.
    let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
    // SAFETY: TIOCGWINSZ writes a winsize to the valid pointer for the provided fd.
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(TerminalSize {
        rows: size.ws_row.max(1),
        cols: size.ws_col.max(1),
        pixel_width: size.ws_xpixel,
        pixel_height: size.ws_ypixel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_arguments_are_shell_safe() {
        assert_eq!(
            shell_command(&["echo".into(), "hello world".into()]),
            "echo 'hello world'"
        );
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(
            shell_command(&["printf hi | sed s/hi/ok/".into()]),
            "printf hi | sed s/hi/ok/"
        );
    }

    #[test]
    fn parses_all_subcommands() {
        let cli = Cli::parse(["gbuild", "logs", "--follow", "42"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::Logs {
                process_id: 42,
                follow: true
            }
        ));

        let cli = Cli::parse(
            ["gbuild", "run", "--project", "7", "--", "npm", "run", "dev"].map(OsString::from),
        )
        .unwrap();
        let Command::Run(run) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(run.project_id, Some(7));
        assert_eq!(run.command, ["npm", "run", "dev"]);
    }
}
