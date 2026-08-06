//! Command-line client for the authenticated workman daemon control channel.

use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, io,
    os::fd::RawFd,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header},
};
use workman_core::{
    DEFAULT_RELEASES_API, LATEST_RELEASES_API, Process, ProcessKind, ProcessSource, ProcessStatus,
    ProjectId, UpdateChannel, UpdateClient, UpdateInstallReport, install_dir_from_executable,
};
use workmand::{
    DaemonVersion, Discovery, McpClient, McpClientSetup, McpConnectionInfo, Service, UpdateStatus,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const DAEMON_WAIT: Duration = Duration::from_secs(5);
const OUTPUT_POLL: Duration = Duration::from_millis(30);
const MAX_OUTPUT_CHUNK: usize = 64 * 1024;
const HELLO_REQUEST_ID: &str = "__workman_cli_hello__";
const HELLO_TIMEOUT: Duration = Duration::from_millis(750);

const HELP: &str = "\
Usage: wrk [--data-dir PATH] [--daemon PATH] [--version] [--update [--check] [--channel stable|latest] [--key KEY]] [COMMAND]\n\
\n\
Commands:\n\
  (none)\n\
      Register the current directory if needed, sync workman.yml, and show status.\n\
  add [PATH]\n\
      Register PATH (default: current directory) as a project.\n\
  up [--project ID]\n\
      Start trusted command processes for the current project.\n\
  down [--project ID]\n\
      Stop trusted command processes for the current project.\n\
  app\n\
      Launch the workman desktop app.\n\
  update [--check] [--channel stable|latest] [--key KEY]\n\
      Check Workman's release host and securely update workman and workmand. Stable is the default;\n\
      latest includes prereleases. --check reports only. --key overrides the configured download key.\n\
  mcp-setup [--client claude|codex|gemini|opencode|generic] [--run]\n\
      Print setup for one MCP client, or all supported clients by default.\n\
      --run executes the Claude setup command.\n\
  run [--project ID] [--name NAME] [--cwd PATH] -- <command...>\n\
      Create and start a durable command process. The current project or\n\
      WORKMAN_PROJECT_ID is used when --project is absent.\n\
  agent --tool ID [--project ID] [--name NAME] [-- <agent args...>]\n\
      Launch a registered agent through the daemon's per-launch MCP wiring.\n\
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
    if matches!(cli.command, Command::Version) {
        println!("workman {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if let Command::Update {
        check_only,
        channel,
        key,
    } = &cli.command
    {
        let data_dir = cli.data_dir.unwrap_or_else(workmand::default_data_dir);
        return self_update(&data_dir, *check_only, *channel, key.as_deref()).await;
    }

    let data_dir = cli.data_dir.unwrap_or_else(workmand::default_data_dir);
    let daemon = daemon_executable(cli.daemon);

    if matches!(&cli.command, Command::App) {
        return launch_app(&data_dir, &daemon).await;
    }
    if let Command::McpSetup { run, client } = &cli.command {
        return mcp_setup(&data_dir, &daemon, *client, *run).await;
    }

    let mut client = Client::connect(&data_dir, &daemon).await?;

    match cli.command {
        Command::Status => status(&mut client).await,
        Command::Add { path } => add(&mut client, path).await,
        Command::Up { project_id } => set_commands(&mut client, project_id, true).await,
        Command::Down { project_id } => set_commands(&mut client, project_id, false).await,
        Command::Run(options) => run_command(&mut client, options).await,
        Command::Agent(options) => run_agent(&mut client, options).await,
        Command::Ps { project_id } => ps(&mut client, project_id).await,
        Command::Logs { process_id, follow } => logs(&mut client, process_id, follow).await,
        Command::Attach { process_id } => attach(&mut client, process_id).await,
        Command::Stop { process_id } => stop(&mut client, process_id).await,
        Command::App
        | Command::McpSetup { .. }
        | Command::Update { .. }
        | Command::Help
        | Command::Version => {
            unreachable!()
        }
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
    Status,
    Add {
        path: PathBuf,
    },
    Up {
        project_id: Option<ProjectId>,
    },
    Down {
        project_id: Option<ProjectId>,
    },
    App,
    Update {
        check_only: bool,
        channel: UpdateChannel,
        key: Option<String>,
    },
    McpSetup {
        run: bool,
        client: Option<McpClient>,
    },
    Run(RunOptions),
    Agent(AgentOptions),
    Ps {
        project_id: Option<ProjectId>,
    },
    Logs {
        process_id: i64,
        follow: bool,
    },
    Attach {
        process_id: i64,
    },
    Stop {
        process_id: i64,
    },
    Help,
    Version,
}

#[derive(Debug)]
struct RunOptions {
    project_id: Option<ProjectId>,
    name: Option<String>,
    cwd: Option<PathBuf>,
    command: Vec<String>,
}

#[derive(Debug)]
struct AgentOptions {
    project_id: Option<ProjectId>,
    agent_tool_id: i64,
    name: Option<String>,
    extra_args: Vec<String>,
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
                break Command::Status;
            };
            match arg.as_str() {
                "--data-dir" => data_dir = Some(PathBuf::from(next_value(&mut args, &arg)?)),
                "--daemon" => daemon = Some(PathBuf::from(next_value(&mut args, &arg)?)),
                "--help" | "-h" | "help" => break Command::Help,
                "--version" | "-V" => {
                    require_no_args(args, "--version")?;
                    break Command::Version;
                }
                "--update" | "update" => break parse_update(args)?,
                "add" => break parse_add(args)?,
                "up" => break parse_project_action(args, true)?,
                "down" => break parse_project_action(args, false)?,
                "app" => {
                    require_no_args(args, "app")?;
                    break Command::App;
                }
                "mcp-setup" => break parse_mcp_setup(args)?,
                "run" => break parse_run(args)?,
                "agent" => break parse_agent(args)?,
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

fn parse_update(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let mut check_only = false;
    let mut channel = UpdateChannel::Stable;
    let mut key = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" if !check_only => check_only = true,
            "--channel" => {
                channel = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| cli_error("--channel must be stable or latest"))?;
            }
            "--key" if key.is_none() => key = Some(next_value(&mut args, &arg)?),
            _ => return Err(cli_error(format!("unknown update option {arg:?}"))),
        }
    }
    Ok(Command::Update {
        check_only,
        channel,
        key,
    })
}

fn parse_add(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let path = args.next().map(PathBuf::from).unwrap_or_else(|| ".".into());
    if args.next().is_some() {
        return Err(cli_error("add accepts at most one path"));
    }
    Ok(Command::Add { path })
}

fn parse_project_action(mut args: impl Iterator<Item = String>, start: bool) -> Result<Command> {
    let mut project_id = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project_id = Some(parse_id(&next_value(&mut args, &arg)?, "project")?),
            _ => return Err(cli_error(format!("unknown option {arg:?}"))),
        }
    }
    Ok(if start {
        Command::Up { project_id }
    } else {
        Command::Down { project_id }
    })
}

fn parse_mcp_setup(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let mut run = false;
    let mut client = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--run" => run = true,
            "--client" => {
                if client.is_some() {
                    return Err(cli_error("--client may only be specified once"));
                }
                let value = next_value(&mut args, &arg)?;
                client = Some(McpClient::parse(&value).ok_or_else(|| {
                    cli_error(format!(
                        "unknown MCP client {value:?}; expected claude, codex, gemini, opencode, or generic"
                    ))
                })?);
            }
            _ => return Err(cli_error(format!("unknown mcp-setup option {arg:?}"))),
        }
    }
    Ok(Command::McpSetup { run, client })
}

fn require_no_args(mut args: impl Iterator<Item = String>, command: &str) -> Result<()> {
    if args.next().is_some() {
        return Err(cli_error(format!("{command} does not accept arguments")));
    }
    Ok(())
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

fn parse_agent(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let mut project_id = None;
    let mut agent_tool_id = None;
    let mut name = None;
    let mut extra_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                extra_args.extend(args);
                break;
            }
            "--project" => {
                project_id = Some(parse_id(&next_value(&mut args, &arg)?, "project")?);
            }
            "--tool" => {
                if agent_tool_id.is_some() {
                    return Err(cli_error("--tool may only be specified once"));
                }
                agent_tool_id = Some(parse_id(&next_value(&mut args, &arg)?, "agent tool")?);
            }
            "--name" => name = Some(next_value(&mut args, &arg)?),
            _ => return Err(cli_error(format!("unknown agent option {arg:?}"))),
        }
    }
    Ok(Command::Agent(AgentOptions {
        project_id,
        agent_tool_id: agent_tool_id.ok_or_else(|| cli_error("agent requires --tool ID"))?,
        name,
        extra_args,
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

#[derive(Clone, Debug, Deserialize)]
struct ProjectView {
    id: ProjectId,
    path: String,
    name: String,
    display_name: Option<String>,
    status: String,
}

impl ProjectView {
    fn label(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }
}

async fn status(client: &mut Client) -> Result<()> {
    let cwd = canonical_directory(&env::current_dir()?)?;
    let project = match project_for_path(client, &cwd).await? {
        Some(project) => project,
        None => register_project(client, &cwd).await?.0,
    };
    sync_project(client, project.id).await?;
    let project = project_by_id(client, project.id).await?;
    show_status(client, &project).await
}

async fn add(client: &mut Client, path: PathBuf) -> Result<()> {
    let path = canonical_directory(&path)?;
    let (project, created) = register_project(client, &path).await?;
    sync_project(client, project.id).await?;
    let project = project_by_id(client, project.id).await?;
    println!();
    if created {
        println!("  ✓ Added to workman");
    } else {
        println!("  ✓ Already registered");
    }
    print_field("Project", project.label());
    print_field("Path", &display_path(Path::new(&project.path)));
    println!();
    Ok(())
}

async fn register_project(client: &mut Client, path: &Path) -> Result<(ProjectView, bool)> {
    let projects = list_projects(client).await?;
    if let Some(project) = projects
        .into_iter()
        .find(|project| Path::new(&project.path) == path)
    {
        return Ok((project, false));
    }
    let projects: Vec<ProjectView> = client
        .rpc("projects.register", json!({ "path": path }))
        .await?;
    let project = projects
        .into_iter()
        .find(|project| Path::new(&project.path) == path)
        .ok_or_else(|| cli_error("daemon registered the directory but did not return it"))?;
    Ok((project, true))
}

async fn sync_project(client: &mut Client, project_id: ProjectId) -> Result<()> {
    let _: Value = client
        .rpc("config.sync", json!({ "project_id": project_id }))
        .await?;
    Ok(())
}

async fn list_projects(client: &mut Client) -> Result<Vec<ProjectView>> {
    client.rpc("projects.list", json!({})).await
}

async fn project_for_path(client: &mut Client, path: &Path) -> Result<Option<ProjectView>> {
    let mut projects = list_projects(client)
        .await?
        .into_iter()
        .filter(|project| path.starts_with(Path::new(&project.path)))
        .collect::<Vec<_>>();
    projects.sort_by_key(|project| Path::new(&project.path).components().count());
    Ok(projects.pop())
}

async fn project_by_id(client: &mut Client, project_id: ProjectId) -> Result<ProjectView> {
    list_projects(client)
        .await?
        .into_iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| cli_error(format!("project {project_id} was not found")))
}

async fn resolve_project_id(
    client: &mut Client,
    explicit: Option<ProjectId>,
    cwd: &Path,
) -> Result<ProjectId> {
    if let Some(id) = explicit {
        return Ok(id);
    }
    if let Ok(value) = env::var("WORKMAN_PROJECT_ID") {
        return parse_id(&value, "project");
    }
    if let Some(project) = project_for_path(client, cwd).await? {
        return Ok(project.id);
    }
    Err(cli_error(format!(
        "no Workman project contains {}; run `wrk` there to add it",
        display_path(cwd)
    )))
}

async fn set_commands(client: &mut Client, explicit: Option<ProjectId>, start: bool) -> Result<()> {
    let cwd = canonical_directory(&env::current_dir()?)?;
    let project_id = resolve_project_id(client, explicit, &cwd).await?;
    let project = project_by_id(client, project_id).await?;
    sync_project(client, project_id).await?;
    let processes: Vec<Process> = client
        .rpc("process.list", json!({ "project_id": project_id }))
        .await?;
    let untrusted = processes
        .iter()
        .filter(|process| process.kind == ProcessKind::Command && !is_trusted(process))
        .count();
    let mut changed = 0;
    for process in processes.into_iter().filter(|process| {
        process.kind == ProcessKind::Command
            && is_trusted(process)
            && if start {
                !is_active(process.status)
            } else {
                is_active(process.status)
            }
    }) {
        let method = if start {
            "process.start"
        } else {
            "process.stop"
        };
        let _: Process = client
            .rpc(method, json!({ "process_id": process.id }))
            .await?;
        changed += 1;
    }

    println!();
    println!(
        "  ✓ {} {} command{}",
        if start { "Started" } else { "Stopped" },
        changed,
        if changed == 1 { "" } else { "s" }
    );
    if untrusted > 0 {
        println!(
            "  ! {untrusted} command{} awaiting trust — review in `wrk app`",
            if untrusted == 1 { " is" } else { "s are" }
        );
    }
    let project = project_by_id(client, project.id).await?;
    show_status(client, &project).await
}

fn is_trusted(process: &Process) -> bool {
    process.source != ProcessSource::Yml || process.trust_hash.is_some()
}

async fn show_status(client: &mut Client, project: &ProjectView) -> Result<()> {
    let processes: Vec<Process> = client
        .rpc("process.list", json!({ "project_id": project.id }))
        .await?;
    let services: Vec<Service> = client
        .rpc("services.list", json!({ "project_id": project.id }))
        .await
        .unwrap_or_default();

    println!();
    println!("  {} · workspace status", project.label());
    println!();
    print_field("Project", project.label());
    print_field("Path", &display_path(Path::new(&project.path)));
    print_field(
        "Daemon",
        &format!("✓ healthy · 127.0.0.1:{}", client.discovery.port),
    );
    print_field("State", &project.status);
    println!();
    println!("  PROCESSES");
    if processes.is_empty() {
        println!("    · none — add commands in workman.yml or run `wrk app`");
    } else {
        for process in &processes {
            let marker = match process.status {
                ProcessStatus::Running => "●",
                ProcessStatus::Starting => "◐",
                ProcessStatus::Crashed => "!",
                ProcessStatus::Stopped | ProcessStatus::Exited => "○",
            };
            let trust = if is_trusted(process) {
                ""
            } else {
                " · review"
            };
            println!(
                "    {marker} {:<10} {}{}",
                process.status, process.name, trust
            );
        }
    }

    let urls = services
        .iter()
        .flat_map(|service| service.urls.iter())
        .collect::<Vec<_>>();
    if !urls.is_empty() {
        println!();
        println!("  SERVICES");
        for url in urls {
            println!("    ↗ {url}");
        }
    }
    println!();
    Ok(())
}

fn print_field(label: &str, value: &str) {
    println!("  {label:<8} {value}");
}

fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .map_err(|error| cli_error(format!("cannot open {}: {error}", path.display())))?;
    if !canonical.is_dir() {
        return Err(cli_error(format!("not a directory: {}", path.display())));
    }
    Ok(canonical)
}

fn display_path(path: &Path) -> String {
    if let Some(home) = env::var_os("HOME").map(PathBuf::from)
        && let Ok(relative) = path.strip_prefix(home)
    {
        return if relative.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", relative.display())
        };
    }
    path.display().to_string()
}

async fn run_command(client: &mut Client, options: RunOptions) -> Result<()> {
    let cwd = options
        .cwd
        .unwrap_or(env::current_dir()?)
        .canonicalize()
        .map_err(|error| cli_error(format!("invalid working directory: {error}")))?;
    let project_id = resolve_project_id(client, options.project_id, &cwd).await?;
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

#[derive(Debug, Deserialize)]
struct SpawnAgentReceipt {
    process_id: i64,
    project_id: ProjectId,
    name: String,
}

async fn run_agent(client: &mut Client, options: AgentOptions) -> Result<()> {
    let cwd = canonical_directory(&env::current_dir()?)?;
    let project_id = resolve_project_id(client, options.project_id, &cwd).await?;
    let spawned: SpawnAgentReceipt = client
        .rpc(
            "agents.spawn",
            json!({
                "project_id": project_id,
                "agent_tool_id": options.agent_tool_id,
                "name": options.name,
                "extra_args": options.extra_args,
            }),
        )
        .await?;
    println!(
        "Started agent {} ({}) in project {}",
        spawned.process_id, spawned.name, spawned.project_id
    );
    Ok(())
}

async fn ps(client: &mut Client, project_id: Option<ProjectId>) -> Result<()> {
    let project_id = if project_id.is_some() || env::var_os("WORKMAN_PROJECT_ID").is_some() {
        Some(
            resolve_project_id(
                client,
                project_id,
                &canonical_directory(&env::current_dir()?)?,
            )
            .await?,
        )
    } else {
        let cwd = canonical_directory(&env::current_dir()?)?;
        project_for_path(client, &cwd)
            .await?
            .map(|project| project.id)
    };
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
                "workman: output before byte {} is no longer retained",
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

fn render_mcp_setup(connection: &McpConnectionInfo, client: Option<McpClient>) -> String {
    if let Some(client) = client {
        return render_mcp_client_setup(
            connection
                .setup(client)
                .expect("every supported MCP client has a setup"),
        );
    }

    let sections = connection
        .setups
        .iter()
        .map(|setup| {
            format!(
                "{}\n{}\n{}",
                setup.label,
                "-".repeat(setup.label.len()),
                render_mcp_client_setup(setup).trim_end()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{sections}\n")
}

fn render_mcp_client_setup(setup: &McpClientSetup) -> String {
    if setup.fields.len() == 1 {
        return format!("{}\n", setup.fields[0].value);
    }
    let fields = setup
        .fields
        .iter()
        .map(|field| format!("{}:\n{}", field.label, field.value))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{fields}\n")
}

async fn mcp_setup(
    data_dir: &Path,
    daemon: &Path,
    client: Option<McpClient>,
    run: bool,
) -> Result<()> {
    if run && client.is_some_and(|client| client != McpClient::Claude) {
        return Err(cli_error(
            "--run is only available with --client claude (or with the default all-client output)",
        ));
    }
    let discovery = workmand::discover_or_spawn(data_dir, daemon, DAEMON_WAIT).await?;
    let connection = workmand::mcp_connection_info(&discovery);
    print!("{}", render_mcp_setup(&connection, client));
    if !run {
        return Ok(());
    }

    let authorization = format!("Authorization: Bearer {}", connection.token);
    let args = [
        "mcp",
        "add",
        "--transport",
        "http",
        "workman",
        connection.endpoint.as_str(),
        "--header",
        authorization.as_str(),
    ];
    let status = ProcessCommand::new("claude")
        .args(args)
        .status()
        .map_err(|error| cli_error(format!("could not run Claude CLI MCP setup: {error}")))?;
    if !status.success() {
        return Err(cli_error(format!(
            "Claude CLI MCP setup exited with {status}"
        )));
    }
    Ok(())
}

async fn launch_app(data_dir: &Path, daemon: &Path) -> Result<()> {
    let target = desktop_launch_target().ok_or_else(|| {
        cli_error("could not find workman-desktop; run the installer again or set WORKMAN_DESKTOP")
    })?;
    let client = Client::connect(data_dir, daemon).await?;
    match client.daemon_version.as_ref() {
        Some(version) => println!(
            "workman daemon v{} · build {} · control protocol {}",
            version.version, version.build_id, version.control_protocol_version
        ),
        None => println!("workman daemon legacy · no version handshake"),
    }
    drop(client);
    match target {
        DesktopLaunchTarget::Bundle(bundle) => launch_macos_bundle(&bundle, data_dir, daemon)?,
        DesktopLaunchTarget::Executable(executable) => {
            #[cfg(target_os = "macos")]
            eprintln!(
                "note: Workman.app was not found; launching {} directly as a development fallback (Dock branding is unavailable)",
                executable.display()
            );
            let child = ProcessCommand::new(&executable)
                .env("WORKMAN_DATA_DIR", data_dir)
                .env("WORKMAN_DAEMON_BIN", daemon)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| {
                    cli_error(format!(
                        "could not launch {}: {error}",
                        executable.display()
                    ))
                })?;
            println!("✓ Opened workman (pid {})", child.id());
        }
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
enum DesktopLaunchTarget {
    Bundle(PathBuf),
    Executable(PathBuf),
}

fn desktop_launch_target() -> Option<DesktopLaunchTarget> {
    if let Some(explicit) = env::var_os("WORKMAN_DESKTOP") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(DesktopLaunchTarget::Executable(path));
        }
    }
    let current = env::current_exe().ok()?;
    let directory = current.parent()?;

    #[cfg(target_os = "macos")]
    if let Some(bundle) = macos_app_bundle_from(
        &current,
        env::var_os("HOME").as_deref().map(Path::new),
        Path::new("/Applications"),
    ) {
        return Some(DesktopLaunchTarget::Bundle(bundle));
    }

    [
        directory.join("workman-desktop"),
        directory.join("bundle/macos/Workman.app/Contents/MacOS/workman-desktop"),
        directory.join("bundle/appimage/workman-desktop"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .map(DesktopLaunchTarget::Executable)
}

#[cfg(any(target_os = "macos", test))]
fn macos_app_bundle_from(
    current: &Path,
    home: Option<&Path>,
    system_applications: &Path,
) -> Option<PathBuf> {
    let directory = current.parent()?;
    let mut candidates = vec![directory.join("bundle/macos/Workman.app")];
    if let Some(package_root) = directory.parent() {
        candidates.push(package_root.join("Workman.app"));
    }
    candidates.push(directory.join("Workman.app"));
    if let Some(home) = home {
        candidates.push(home.join("Applications/Workman.app"));
    }
    candidates.push(system_applications.join("Workman.app"));
    candidates
        .into_iter()
        .find(|bundle| bundle.is_dir() && bundle.join("Contents/MacOS/workman-desktop").is_file())
}

#[cfg(target_os = "macos")]
fn launch_macos_bundle(bundle: &Path, data_dir: &Path, daemon: &Path) -> Result<()> {
    let open = Path::new("/usr/bin/open");
    let supports_env = ProcessCommand::new(open)
        .arg("-h")
        .output()
        .is_ok_and(|output| {
            open_help_supports_env(&output.stdout) || open_help_supports_env(&output.stderr)
        });
    let status = ProcessCommand::new(open)
        .args(macos_open_args(bundle, data_dir, daemon, supports_env))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| cli_error(format!("could not open {}: {error}", bundle.display())))?;
    if !status.success() {
        return Err(cli_error(format!(
            "could not open {} through LaunchServices: {status}",
            bundle.display()
        )));
    }
    println!("✓ Opened Workman.app through LaunchServices");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn launch_macos_bundle(_bundle: &Path, _data_dir: &Path, _daemon: &Path) -> Result<()> {
    unreachable!("bundle launch targets are only resolved on macOS")
}

#[cfg(any(target_os = "macos", test))]
fn macos_open_args(
    bundle: &Path,
    data_dir: &Path,
    daemon: &Path,
    supports_env: bool,
) -> Vec<OsString> {
    let mut args = vec![OsString::from("-a"), bundle.as_os_str().to_owned()];
    if supports_env {
        args.extend([
            OsString::from("--env"),
            environment_assignment("WORKMAN_DATA_DIR", data_dir),
            OsString::from("--env"),
            environment_assignment("WORKMAN_DAEMON_BIN", daemon),
        ]);
    } else {
        args.extend([
            OsString::from("--args"),
            OsString::from("--workman-data-dir"),
            data_dir.as_os_str().to_owned(),
            OsString::from("--workman-daemon-bin"),
            daemon.as_os_str().to_owned(),
        ]);
    }
    args
}

#[cfg(any(target_os = "macos", test))]
fn environment_assignment(name: &str, value: &Path) -> OsString {
    let mut assignment = OsString::from(name);
    assignment.push("=");
    assignment.push(value);
    assignment
}

#[cfg(any(target_os = "macos", test))]
fn open_help_supports_env(output: &[u8]) -> bool {
    String::from_utf8_lossy(output).contains("--env")
}

fn daemon_executable(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(path) = env::var_os("WORKMAN_DAEMON") {
        return PathBuf::from(path);
    }
    if let Ok(current) = env::current_exe()
        && let Some(sibling) = sibling_daemon_executable(&current)
    {
        return sibling;
    }
    PathBuf::from("workmand")
}

fn sibling_daemon_executable(current: &Path) -> Option<PathBuf> {
    // The one-release v0.1.0 updater bridge installs the Workman daemon beneath the old filename.
    // Prefer the canonical sibling, but keep that upgraded installation usable.
    ["workmand", "awmd"]
        .into_iter()
        .map(|name| current.with_file_name(name))
        .find(|path| path.is_file())
}

async fn self_update(
    data_dir: &Path,
    check_only: bool,
    channel: UpdateChannel,
    explicit_key: Option<&str>,
) -> Result<()> {
    let api_url = match channel {
        UpdateChannel::Stable => {
            env::var("WORKMAN_RELEASES_API_URL").unwrap_or_else(|_| DEFAULT_RELEASES_API.to_owned())
        }
        UpdateChannel::Latest => env::var("WORKMAN_LATEST_RELEASES_API_URL")
            .unwrap_or_else(|_| LATEST_RELEASES_API.to_owned()),
    };
    let update_key = workmand::resolve_update_key(explicit_key)?;
    let updater = UpdateClient::new_for_channel(api_url, channel)?.with_key(&update_key)?;
    let mut daemon_client = if let Ok(discovery) = Discovery::read(data_dir)
        && workmand::probe(&discovery).await
    {
        Some(Client::connect_discovery(discovery).await?)
    } else {
        None
    };
    let check = if let Some(client) = daemon_client.as_mut() {
        let _: UpdateStatus = client
            .rpc("daemon.update_preferences", json!({ "channel": channel }))
            .await?;
        client
            .rpc::<UpdateStatus>(
                "daemon.update_check",
                json!({ "force": true, "key": &update_key }),
            )
            .await?
            .check
    } else {
        updater.check(env!("CARGO_PKG_VERSION")).await?
    };
    println!("Channel: {}", check.channel);
    println!("Current: {}", check.current);
    println!("Latest:  {}", check.latest);
    if !check.notes.trim().is_empty() {
        println!("\n{}", check.notes.trim());
    }
    if !check.available {
        println!("\nworkman is up to date.");
        return Ok(());
    }
    println!("\nRelease: {}", check.url);
    if check_only {
        return Ok(());
    }

    let report = if let Some(client) = daemon_client.as_mut() {
        eprintln!(
            "workman: warning: updating restarts workmand and stops all running project processes"
        );
        client
            .rpc("daemon.update_apply", json!({ "key": &update_key }))
            .await?
    } else {
        let install_dir = match env::var_os("WORKMAN_UPDATE_INSTALL_DIR") {
            Some(path) => PathBuf::from(path),
            None => install_dir_from_executable(env::current_exe()?)?,
        };
        updater.install(&check, install_dir).await?
    };
    print_update_report(&report);
    Ok(())
}

fn print_update_report(report: &UpdateInstallReport) {
    println!("Updated workman {} → {}", report.current, report.latest);
    for path in &report.updated_files {
        println!("  {path}");
    }
    if report.quarantine_cleared {
        println!("Cleared macOS quarantine attributes from the verified update.");
    }
    if let Some(instruction) = &report.desktop_instruction {
        println!("\n{instruction}");
    }
}

struct Client {
    socket: Socket,
    next_id: u64,
    discovery: Discovery,
    daemon_version: Option<DaemonVersion>,
}

impl Client {
    async fn connect(data_dir: &Path, daemon: &Path) -> Result<Self> {
        let discovery = workmand::discover_or_spawn(data_dir, daemon, DAEMON_WAIT).await?;
        Self::connect_discovery(discovery).await
    }

    async fn connect_discovery(discovery: Discovery) -> Result<Self> {
        let mut request = format!("ws://127.0.0.1:{}/ws", discovery.port).into_client_request()?;
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {}", discovery.token).parse()?,
        );
        let (mut socket, _) = connect_async(request).await?;
        let daemon_version = negotiate_daemon_version(&mut socket).await;
        Ok(Self {
            socket,
            next_id: 1,
            discovery,
            daemon_version,
        })
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

async fn negotiate_daemon_version(socket: &mut Socket) -> Option<DaemonVersion> {
    socket
        .send(Message::Text(
            json!({ "id": HELLO_REQUEST_ID, "method": "daemon.hello", "params": {} })
                .to_string()
                .into(),
        ))
        .await
        .ok()?;

    timeout(HELLO_TIMEOUT, async {
        loop {
            match socket.next().await? {
                Ok(Message::Text(text)) => {
                    let response = serde_json::from_str::<Value>(&text).ok()?;
                    if response.get("id").and_then(Value::as_str) == Some(HELLO_REQUEST_ID) {
                        if response.get("ok").and_then(Value::as_bool) != Some(true) {
                            return None;
                        }
                        return response
                            .get("result")
                            .cloned()
                            .and_then(|result| serde_json::from_value(result).ok());
                    }
                }
                Ok(Message::Ping(bytes)) => {
                    socket.send(Message::Pong(bytes)).await.ok()?;
                }
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    })
    .await
    .ok()
    .flatten()
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

    fn create_test_app_bundle(bundle: &Path) {
        let executable = bundle.join("Contents/MacOS/workman-desktop");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(executable, b"fixture").unwrap();
    }

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
    fn macos_bundle_resolution_covers_build_and_installed_layouts() {
        let root = tempfile::tempdir().unwrap();
        let wrk = root.path().join("bin/wrk");
        let bundled = root.path().join("bin/bundle/macos/Workman.app");
        let installed = root.path().join("Workman.app");
        create_test_app_bundle(&installed);
        create_test_app_bundle(&bundled);

        assert_eq!(
            macos_app_bundle_from(&wrk, None, &root.path().join("Applications")),
            Some(bundled)
        );
    }

    #[test]
    fn macos_bundle_resolution_falls_back_to_standard_applications() {
        let root = tempfile::tempdir().unwrap();
        let wrk = root.path().join("bin/wrk");
        let home = root.path().join("home");
        let system_applications = root.path().join("Applications");
        let user_bundle = home.join("Applications/Workman.app");
        let system_bundle = system_applications.join("Workman.app");
        create_test_app_bundle(&system_bundle);
        create_test_app_bundle(&user_bundle);

        assert_eq!(
            macos_app_bundle_from(&wrk, Some(&home), &system_applications),
            Some(user_bundle)
        );
    }

    #[test]
    fn macos_open_uses_launchservices_env_without_requesting_a_new_instance() {
        let bundle = Path::new("/tmp/Workman.app");
        let args = macos_open_args(
            bundle,
            Path::new("/tmp/workman data"),
            Path::new("/tmp/workmand"),
            true,
        );
        assert_eq!(
            args,
            [
                OsString::from("-a"),
                bundle.as_os_str().to_owned(),
                OsString::from("--env"),
                OsString::from("WORKMAN_DATA_DIR=/tmp/workman data"),
                OsString::from("--env"),
                OsString::from("WORKMAN_DAEMON_BIN=/tmp/workmand"),
            ]
        );
        assert!(!args.contains(&OsString::from("-n")));
    }

    #[test]
    fn macos_open_falls_back_to_private_launch_arguments() {
        let args = macos_open_args(
            Path::new("/tmp/Workman.app"),
            Path::new("/tmp/data"),
            Path::new("/tmp/workmand"),
            false,
        );
        assert!(args.contains(&OsString::from("--args")));
        assert!(args.contains(&OsString::from("--workman-data-dir")));
        assert!(args.contains(&OsString::from("--workman-daemon-bin")));
        assert!(open_help_supports_env(b"open options: --env VAR"));
        assert!(!open_help_supports_env(b"open options: --args"));
    }

    #[test]
    fn parses_all_subcommands() {
        let cli = Cli::parse(["wrk"].map(OsString::from)).unwrap();
        assert!(matches!(cli.command, Command::Status));

        let cli = Cli::parse(["wrk", "--version"].map(OsString::from)).unwrap();
        assert!(matches!(cli.command, Command::Version));
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.2");

        let cli = Cli::parse(["wrk", "update", "--check"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                check_only: true,
                channel: UpdateChannel::Stable,
                key: None,
            }
        ));
        let cli =
            Cli::parse(["wrk", "update", "--channel", "latest", "--check"].map(OsString::from))
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                check_only: true,
                channel: UpdateChannel::Latest,
                key: None,
            }
        ));
        let cli = Cli::parse(["wrk", "--update"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                check_only: false,
                channel: UpdateChannel::Stable,
                key: None,
            }
        ));
        let cli =
            Cli::parse(["wrk", "update", "--key", "friends-key", "--check"].map(OsString::from))
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Update {
                check_only: true,
                channel: UpdateChannel::Stable,
                key: Some(ref key),
            } if key == "friends-key"
        ));
        assert!(Cli::parse(["wrk", "update", "--key"].map(OsString::from)).is_err());
        assert!(
            Cli::parse(["wrk", "update", "--key", "one", "--key", "two"].map(OsString::from))
                .is_err()
        );
        assert!(Cli::parse(["wrk", "update", "--channel", "edge"].map(OsString::from)).is_err());

        let cli = Cli::parse(["wrk", "add"].map(OsString::from)).unwrap();
        assert!(matches!(cli.command, Command::Add { path } if path == Path::new(".")));

        let cli = Cli::parse(["wrk", "up", "--project", "8"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::Up {
                project_id: Some(8)
            }
        ));

        let cli =
            Cli::parse(["wrk", "mcp-setup", "--client", "codex"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::McpSetup {
                run: false,
                client: Some(McpClient::Codex)
            }
        ));

        let cli = Cli::parse(["wrk", "mcp-setup", "--run"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::McpSetup {
                run: true,
                client: None
            }
        ));
        assert!(
            Cli::parse(["wrk", "mcp-setup", "--client", "unknown"].map(OsString::from)).is_err()
        );

        let cli = Cli::parse(["wrk", "logs", "--follow", "42"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::Logs {
                process_id: 42,
                follow: true
            }
        ));

        let cli = Cli::parse(
            ["wrk", "run", "--project", "7", "--", "npm", "run", "dev"].map(OsString::from),
        )
        .unwrap();
        let Command::Run(run) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(run.project_id, Some(7));
        assert_eq!(run.command, ["npm", "run", "dev"]);

        let cli = Cli::parse(
            [
                "wrk",
                "agent",
                "--tool",
                "6",
                "--project",
                "7",
                "--name",
                "reviewer",
                "--",
                "--model",
                "gpt-test",
            ]
            .map(OsString::from),
        )
        .unwrap();
        let Command::Agent(agent) = cli.command else {
            panic!("expected agent command");
        };
        assert_eq!(agent.agent_tool_id, 6);
        assert_eq!(agent.project_id, Some(7));
        assert_eq!(agent.name.as_deref(), Some("reviewer"));
        assert_eq!(agent.extra_args, ["--model", "gpt-test"]);
        assert!(Cli::parse(["wrk", "agent"].map(OsString::from)).is_err());
    }

    #[test]
    fn renders_all_or_one_mcp_client_setup() {
        let connection = workmand::mcp_connection_info(&Discovery {
            port: 41731,
            token: "test-token".into(),
            pid: 42,
        });

        let all = render_mcp_setup(&connection, None);
        for label in ["Claude Code", "Codex", "Gemini CLI", "OpenCode", "Generic"] {
            assert!(all.contains(label), "missing {label} section:\n{all}");
        }
        assert!(all.contains("mcpServers"));
        assert!(all.contains("env_http_headers"));
        assert!(all.contains("Header value:\nBearer test-token"));

        let claude = render_mcp_setup(&connection, Some(McpClient::Claude));
        assert!(claude.starts_with("claude mcp add --transport http workman "));
        assert!(!claude.contains("Codex\n"));

        let codex = render_mcp_setup(&connection, Some(McpClient::Codex));
        assert!(codex.contains("export WORKMAN_MCP_AUTHORIZATION='Bearer test-token'"));
        assert!(codex.contains("[mcp_servers.workman]"));
        assert!(
            codex.contains(
                "env_http_headers = { \"Authorization\" = \"WORKMAN_MCP_AUTHORIZATION\" }"
            )
        );
    }

    #[test]
    fn daemon_sibling_prefers_workmand_and_falls_back_to_transitional_awmd() {
        let directory = tempfile::tempdir().unwrap();
        let cli = directory.path().join("awm");
        let legacy = directory.path().join("awmd");
        std::fs::write(&legacy, "legacy filename containing workmand").unwrap();
        assert_eq!(sibling_daemon_executable(&cli), Some(legacy));

        let canonical = directory.path().join("workmand");
        std::fs::write(&canonical, "workmand").unwrap();
        assert_eq!(sibling_daemon_executable(&cli), Some(canonical));
    }
}
