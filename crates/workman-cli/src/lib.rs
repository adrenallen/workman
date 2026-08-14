//! Command-line client for the authenticated workman daemon control channel.

use std::{
    borrow::Cow,
    env,
    error::Error,
    ffi::{OsStr, OsString},
    fmt, io,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    time::Duration,
};

#[cfg(unix)]
use std::os::fd::RawFd;

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
    Profile, ProjectId, UpdateChannel, UpdateClient, UpdateInstallReport, UpdateInstallTarget,
};
use workmand::{
    DaemonVersion, Discovery, McpClient, McpClientSetup, McpConnectionInfo, RuntimeIdentity,
    Service, UpdateStatus,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

// A healthy daemon normally publishes discovery in well under a second, but debug binaries can
// be starved for several seconds while a workspace build is saturating the machine. Keep the
// first `wrk` invocation attached long enough to survive that transient load.
const DAEMON_WAIT: Duration = Duration::from_secs(15);
const OUTPUT_POLL: Duration = Duration::from_millis(30);
const MAX_OUTPUT_CHUNK: usize = 64 * 1024;
const HELLO_REQUEST_ID: &str = "__workman_cli_hello__";
const HELLO_TIMEOUT: Duration = Duration::from_millis(750);
const WORKMAN_DATA_DIR_ENV: &str = "WORKMAN_DATA_DIR";
const REQUIRE_EXPLICIT_DAEMON_ENV: &str = "WORKMAN_REQUIRE_EXPLICIT_DAEMON";

const ROOT_HELP: &str = concat!(
    "wrk ",
    env!("CARGO_PKG_VERSION"),
    " — local workspace and process control\n",
    "Usage: wrk [GLOBAL OPTIONS] [COMMAND] [OPTIONS]\n",
    "\n",
    "Workspace\n",
    "  (no command)  Register or sync the current directory and show status\n",
    "  add           Register a project folder\n",
    "  project       Remove any project from Workman or the local computer\n",
    "  profile       List, create, switch, export, import, or delete profiles\n",
    "\n",
    "Processes\n",
    "  up            Start trusted commands\n",
    "  down          Stop trusted commands\n",
    "  run           Create and start a durable command\n",
    "  agent         Launch a configured agent\n",
    "  ps            List processes\n",
    "  logs          Read or follow process output\n",
    "  attach        Attach to a live process\n",
    "  stop          Stop one process\n",
    "\n",
    "Worktrees\n",
    "  worktree      Compatibility alias for project removal\n",
    "  app           Open the desktop workspace and worktree tools\n",
    "\n",
    "Updates\n",
    "  update        Check for or install Workman updates\n",
    "\n",
    "Daemon\n",
    "  mcp-setup     Print authenticated MCP client setup\n",
    "\n",
    "Misc\n",
    "  help          Show root or command help\n",
    "\n",
    "Global options\n",
    "  --data-dir PATH  Use an isolated Workman data directory\n",
    "  --daemon PATH    Use a specific workmand executable\n",
    "  -h, --help       Show help and exit\n",
    "  -V, --version    Show version and exit\n",
    "\n",
    "Environment\n",
    "  WORKMAN_DATA_DIR=PATH                 Default data directory; --data-dir wins\n",
    "  WORKMAN_REQUIRE_EXPLICIT_DAEMON=1     Refuse implicit default-daemon access\n",
    "\n",
    "Automation\n",
    "  Set the guard and give each worker a fresh data directory. --daemon alone does not isolate\n",
    "  daemon state. Help, version, and update do not target a daemon.\n",
    "\n",
    "Examples\n",
    "  wrk\n",
    "  wrk add ~/Code/my-app\n",
    "  wrk run --name dev -- pnpm dev\n",
    "  wrk logs --follow 42\n",
    "\n",
    "Run `wrk help COMMAND` for command details.\n",
    "Docs: https://github.com/adrenallen/workman\n",
);

const ADD_HELP: &str = r#"wrk add — register a project folder
Usage: wrk add [PATH]

Arguments
  PATH        Project folder; defaults to the current directory

Options
  -h, --help  Show help and exit

Example
  wrk add ~/Code/my-app
"#;

const PROFILE_HELP: &str = r#"wrk profile — manage switchable project/config profiles
Usage: wrk profile COMMAND [OPTIONS]

Examples
  wrk profile list
  wrk profile create NAME [--empty]
  wrk profile switch ID [--stop-running]
  wrk profile export ID PATH
  wrk profile import PATH [--name NAME]
  wrk profile delete ID

Commands
  list     List profiles; * marks the active profile
  create   Snapshot the active profile; --empty creates a vanilla empty profile
  switch   Switch profiles; --stop-running confirms stopping outgoing live processes
  export   Write a secret-free portable JSON archive
  import   Validate an archive fully, then create an inactive profile
  delete   Delete an inactive profile (project-owned data remains with canonical projects)

Options
  -h, --help  Show help and exit
"#;

const UP_HELP: &str = r#"wrk up — start trusted project commands
Usage: wrk up [--project ID]

Options
  --project ID  Target project; defaults to the current project
  -h, --help    Show help and exit
"#;

const DOWN_HELP: &str = r#"wrk down — stop trusted project commands
Usage: wrk down [--project ID]

Options
  --project ID  Target project; defaults to the current project
  -h, --help    Show help and exit
"#;

const APP_HELP: &str = r#"wrk app — open the Workman desktop workspace
Usage: wrk app

Use the desktop app to manage projects, worktrees, processes, and coordination.

Options
  -h, --help  Show help and exit
"#;

const WORKTREE_HELP: &str = r#"wrk worktree — compatibility alias for project removal
Usage: wrk worktree remove [OPTIONS]

Commands
  remove   Unregister the current or selected project from Workman

Remove options
  --project ID             Target project; defaults to the current project
  --delete-local           Also delete the exact local project folder
  --stop-running           Confirm stopping running project processes
  --force                  Permit guarded loss of local/unpublished work
  -h, --help               Show help and exit

Without --delete-local, files stay on your computer. This command has the same
local-only behavior as `wrk project remove` and never changes a remote.
"#;

const PROJECT_HELP: &str = r#"wrk project — manage registered projects
Usage: wrk project remove [OPTIONS]

Commands
  remove   Unregister any project, optionally deleting its exact local folder

Remove options
  --project ID       Target project; defaults to the current project
  --delete-local     Also permanently delete the exact local project folder
  --stop-running     Confirm stopping running project processes
  --force            Permit guarded local loss (dirty/unpublished/dependent worktrees)
  -h, --help         Show help and exit

Without --delete-local, files stay on your computer. Linked Git worktrees use local
`git worktree remove` plus metadata pruning and keep their local branch. Removal never
pushes, fetches, prunes remote refs, or deletes a remote branch.
"#;

const UPDATE_HELP: &str = r#"wrk update — check for or install Workman updates
Usage: wrk update [OPTIONS]

Options
  --check                  Check without installing
  --channel stable|latest  Select stable or prerelease updates
  --key KEY                Override the configured download key
  -h, --help               Show help and exit

Examples
  wrk update --check
  wrk update --channel latest
"#;

const DEV_UPDATE_HELP: &str = r#"wrk-dev update — development installs are rebuilt from source
Usage: wrk-dev update

The dev identity never replaces stable Workman files or downloads a release over the current
working-tree build. Re-run scripts/dev-install.sh from the repository to refresh it.

Options
  -h, --help  Show help and exit
"#;

const MCP_SETUP_HELP: &str = r#"wrk mcp-setup — print authenticated MCP client setup
Usage: wrk mcp-setup [OPTIONS]

Options
  --client CLIENT  Limit output to claude, codex, gemini, opencode, or generic
  --run            Run the Claude setup command
  -h, --help       Show help and exit
"#;

const RUN_HELP: &str = r#"wrk run — create and start a durable command
Usage: wrk run [OPTIONS] [--] COMMAND [ARG...]

Options
  --project ID  Target project; defaults to the current project
  --name NAME   Set the stored process name
  --cwd PATH    Set the process working directory
  -h, --help    Show help and exit

Use `--` before COMMAND when its arguments contain Workman option names.

Examples
  wrk run --name dev -- pnpm dev
  wrk run -- npm --help
"#;

const AGENT_HELP: &str = r#"wrk agent — launch a configured agent
Usage: wrk agent --tool ID [OPTIONS] [-- AGENT_ARG...]

Options
  --tool ID     Required agent-tool ID
  --project ID  Target project; defaults to the current project
  --name NAME   Set the stored agent name
  -h, --help    Show help and exit
"#;

const PS_HELP: &str = r#"wrk ps — list processes
Usage: wrk ps [--project ID]

Options
  --project ID  Limit output to one project
  -h, --help    Show help and exit
"#;

const LOGS_HELP: &str = r#"wrk logs — read or follow process output
Usage: wrk logs [-f|--follow] PROCESS_ID

Options
  -f, --follow  Follow live output until the process exits
  -h, --help    Show help and exit
"#;

const ATTACH_HELP: &str = r#"wrk attach — attach to a live process
Usage: wrk attach PROCESS_ID

Replays saved output, forwards terminal input and resize events, then exits with the process.

Options
  -h, --help  Show help and exit
"#;

const STOP_HELP: &str = r#"wrk stop — stop one process
Usage: wrk stop PROCESS_ID

Options
  -h, --help  Show help and exit
"#;

const HELP_HELP: &str = r#"wrk help — show root or command help
Usage: wrk help [COMMAND]

Example
  wrk help run
"#;

/// Parse process arguments and execute one CLI invocation.
pub async fn run_env() -> Result<()> {
    run(
        env::args_os(),
        env::var_os(WORKMAN_DATA_DIR_ENV),
        env::var_os(REQUIRE_EXPLICIT_DAEMON_ENV),
    )
    .await
}

async fn run(
    args: impl IntoIterator<Item = OsString>,
    data_dir_environment: Option<OsString>,
    require_explicit_environment: Option<OsString>,
) -> Result<()> {
    let identity = RuntimeIdentity::current();
    let cli = Cli::parse(args)?;
    if let Command::Help(topic) = &cli.command {
        print!("{}", help_text_for(identity, *topic));
        return Ok(());
    }
    if matches!(cli.command, Command::Version) {
        println!("{}", version_text(identity));
        return Ok(());
    }

    if let Command::Update {
        check_only,
        channel,
        key,
    } = &cli.command
    {
        if identity.is_dev() {
            println!("{}", dev_update_notice());
            return Ok(());
        }
        let data_dir = resolve_data_dir(cli.data_dir, data_dir_environment);
        return self_update(&data_dir, *check_only, *channel, key.as_deref()).await;
    }

    require_explicit_daemon_target(
        cli.data_dir.as_deref(),
        data_dir_environment.as_deref(),
        require_explicit_environment.as_deref(),
    )?;
    let data_dir = resolve_data_dir(cli.data_dir, data_dir_environment);
    let daemon = daemon_executable(cli.daemon, identity);

    if matches!(&cli.command, Command::App) {
        let config = workmand::user_config_path();
        return launch_app(&data_dir, &config, &daemon, identity).await;
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
        Command::Profile(command) => profile(&mut client, command).await,
        Command::Project(command) | Command::Worktree(command) => {
            project_remove(&mut client, command).await
        }
        Command::App
        | Command::McpSetup { .. }
        | Command::Update { .. }
        | Command::Help(_)
        | Command::Version => {
            unreachable!()
        }
    }
}

fn resolve_data_dir(
    data_dir_option: Option<PathBuf>,
    data_dir_environment: Option<OsString>,
) -> PathBuf {
    data_dir_option
        .or_else(|| data_dir_environment.map(PathBuf::from))
        .unwrap_or_else(workmand::default_data_dir)
}

fn require_explicit_daemon_target(
    data_dir_option: Option<&Path>,
    data_dir_environment: Option<&OsStr>,
    require_explicit_environment: Option<&OsStr>,
) -> Result<()> {
    let guard_enabled = require_explicit_environment == Some(OsStr::new("1"));
    let has_explicit_data_dir =
        data_dir_option.is_some() || data_dir_environment.is_some_and(|value| !value.is_empty());
    if guard_enabled && !has_explicit_data_dir {
        return Err(cli_error(format!(
            "{REQUIRE_EXPLICIT_DAEMON_ENV}=1 blocked implicit default-daemon access; pass --data-dir PATH or set {WORKMAN_DATA_DIR_ENV}=PATH to an isolated directory"
        )));
    }
    Ok(())
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
    Profile(ProfileCommand),
    Project(WorktreeCommand),
    Worktree(WorktreeCommand),
    Help(HelpTopic),
    Version,
}

#[derive(Debug)]
enum ProfileCommand {
    List,
    Create { name: String, copy_current: bool },
    Switch { profile_id: i64, stop_running: bool },
    Export { profile_id: i64, path: PathBuf },
    Import { path: PathBuf, name: Option<String> },
    Delete { profile_id: i64 },
}

#[derive(Debug)]
enum WorktreeCommand {
    Remove {
        project_id: Option<ProjectId>,
        delete_from_disk: bool,
        stop_running: bool,
        force_dirty: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelpTopic {
    Root,
    Add,
    Up,
    Down,
    App,
    Update,
    McpSetup,
    Run,
    Agent,
    Ps,
    Logs,
    Attach,
    Stop,
    Profile,
    Project,
    Worktree,
    Help,
}

impl HelpTopic {
    const SUBCOMMANDS: [(&'static str, Self); 16] = [
        ("add", Self::Add),
        ("up", Self::Up),
        ("down", Self::Down),
        ("app", Self::App),
        ("update", Self::Update),
        ("mcp-setup", Self::McpSetup),
        ("run", Self::Run),
        ("agent", Self::Agent),
        ("ps", Self::Ps),
        ("logs", Self::Logs),
        ("attach", Self::Attach),
        ("stop", Self::Stop),
        ("profile", Self::Profile),
        ("project", Self::Project),
        ("worktree", Self::Worktree),
        ("help", Self::Help),
    ];

    fn command(self) -> Option<&'static str> {
        Self::SUBCOMMANDS
            .iter()
            .find_map(|(command, topic)| (*topic == self).then_some(*command))
    }

    fn for_command(command: &str) -> Option<Self> {
        Self::SUBCOMMANDS
            .iter()
            .find_map(|(name, topic)| (*name == command).then_some(*topic))
    }
}

fn help_text(topic: HelpTopic) -> &'static str {
    match topic {
        HelpTopic::Root => ROOT_HELP,
        HelpTopic::Add => ADD_HELP,
        HelpTopic::Up => UP_HELP,
        HelpTopic::Down => DOWN_HELP,
        HelpTopic::App => APP_HELP,
        HelpTopic::Update => UPDATE_HELP,
        HelpTopic::McpSetup => MCP_SETUP_HELP,
        HelpTopic::Run => RUN_HELP,
        HelpTopic::Agent => AGENT_HELP,
        HelpTopic::Ps => PS_HELP,
        HelpTopic::Logs => LOGS_HELP,
        HelpTopic::Attach => ATTACH_HELP,
        HelpTopic::Stop => STOP_HELP,
        HelpTopic::Profile => PROFILE_HELP,
        HelpTopic::Project => PROJECT_HELP,
        HelpTopic::Worktree => WORKTREE_HELP,
        HelpTopic::Help => HELP_HELP,
    }
}

fn help_text_for(identity: RuntimeIdentity, topic: HelpTopic) -> Cow<'static, str> {
    if !identity.is_dev() {
        return Cow::Borrowed(help_text(topic));
    }
    if topic == HelpTopic::Update {
        return Cow::Borrowed(DEV_UPDATE_HELP);
    }
    Cow::Owned(help_text(topic).replace("wrk ", "wrk-dev "))
}

fn version_text(identity: RuntimeIdentity) -> String {
    if identity.is_dev() {
        format!(
            "workman-dev {} (build {})",
            env!("CARGO_PKG_VERSION"),
            workmand::BUILD_ID
        )
    } else {
        format!("workman {}", env!("CARGO_PKG_VERSION"))
    }
}

fn dev_update_notice() -> &'static str {
    "workman dev install — rebuild from the current working tree with scripts/dev-install.sh; stable Workman was not changed."
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
        let args = args
            .map(|arg| {
                arg.into_string()
                    .map_err(|_| cli_error("arguments must be valid UTF-8"))
            })
            .collect::<Result<Vec<_>>>()?;

        if root_help_requested(&args) {
            return Ok(Self {
                data_dir: None,
                daemon: None,
                command: Command::Help(HelpTopic::Root),
            });
        }

        let mut data_dir = None;
        let mut daemon = None;
        let mut args = args.into_iter();
        let command = loop {
            let Some(arg) = args.next() else {
                break Command::Status;
            };
            match arg.as_str() {
                "--data-dir" if data_dir.is_none() => {
                    data_dir = Some(PathBuf::from(next_value(&mut args, &arg, HelpTopic::Root)?));
                }
                "--daemon" if daemon.is_none() => {
                    daemon = Some(PathBuf::from(next_value(&mut args, &arg, HelpTopic::Root)?));
                }
                "--help" | "-h" => break Command::Help(HelpTopic::Root),
                "help" => break parse_help(args.collect())?,
                "--version" | "-V" => {
                    require_no_args(args.collect(), "--version", HelpTopic::Root)?;
                    break Command::Version;
                }
                "--update" | "update" => break parse_update(args.collect())?,
                "add" => break parse_add(args.collect())?,
                "up" => break parse_project_action(args.collect(), true)?,
                "down" => break parse_project_action(args.collect(), false)?,
                "app" => {
                    let remaining = args.collect::<Vec<_>>();
                    if help_requested(&remaining) {
                        break Command::Help(HelpTopic::App);
                    }
                    require_no_args(remaining, "app", HelpTopic::App)?;
                    break Command::App;
                }
                "mcp-setup" => break parse_mcp_setup(args.collect())?,
                "run" => break parse_run(args.collect())?,
                "agent" => break parse_agent(args.collect())?,
                "ps" => break parse_ps(args.collect())?,
                "logs" => break parse_logs(args.collect())?,
                "attach" => break parse_process_id_command(args.collect(), HelpTopic::Attach)?,
                "stop" => break parse_process_id_command(args.collect(), HelpTopic::Stop)?,
                "profile" => break parse_profile(args.collect())?,
                "project" => break parse_project_remove(args.collect())?,
                "worktree" => break parse_worktree(args.collect())?,
                _ if arg.starts_with('-') => {
                    return Err(unknown_option(HelpTopic::Root, &arg));
                }
                _ => {
                    return Err(usage_error(
                        HelpTopic::Root,
                        format!("unknown command {arg:?}"),
                    ));
                }
            }
        };

        Ok(Self {
            data_dir,
            daemon,
            command,
        })
    }
}

fn root_help_requested(args: &[String]) -> bool {
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => return true,
            "--data-dir" | "--daemon" => {
                if matches!(
                    args.get(index + 1).map(String::as_str),
                    Some("-h" | "--help")
                ) {
                    return true;
                }
                index += 2;
            }
            "--version" | "-V" => {
                return args[index + 1..]
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-h" | "--help"));
            }
            "--update" | "update" | "add" | "up" | "down" | "app" | "mcp-setup" | "run"
            | "agent" | "ps" | "logs" | "attach" | "stop" | "profile" | "project" | "worktree"
            | "help" => {
                return false;
            }
            _ => index += 1,
        }
    }
    false
}

fn help_requested(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
}

fn parse_help(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Help));
    }
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help(HelpTopic::Root));
    };
    if command.starts_with('-') {
        return Err(unknown_option(HelpTopic::Help, &command));
    }
    let topic = HelpTopic::for_command(&command)
        .ok_or_else(|| usage_error(HelpTopic::Help, format!("unknown command {command:?}")))?;
    require_no_args(args.collect(), "help", HelpTopic::Help)?;
    Ok(Command::Help(topic))
}

fn parse_update(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Update));
    }
    let mut args = args.into_iter();
    let mut check_only = false;
    let mut channel = UpdateChannel::Stable;
    let mut key = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" if !check_only => check_only = true,
            "--check" => {
                return Err(usage_error(
                    HelpTopic::Update,
                    "--check may only be specified once",
                ));
            }
            "--channel" => {
                channel = next_value(&mut args, &arg, HelpTopic::Update)?
                    .parse()
                    .map_err(|_| {
                        usage_error(HelpTopic::Update, "--channel must be stable or latest")
                    })?;
            }
            "--key" if key.is_none() => {
                key = Some(next_value(&mut args, &arg, HelpTopic::Update)?);
            }
            "--key" => {
                return Err(usage_error(
                    HelpTopic::Update,
                    "--key may only be specified once",
                ));
            }
            _ => return Err(unknown_option(HelpTopic::Update, &arg)),
        }
    }
    Ok(Command::Update {
        check_only,
        channel,
        key,
    })
}

fn parse_add(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Add));
    }
    let mut args = args.into_iter();
    let path = match args.next() {
        Some(path) if path.starts_with('-') => return Err(unknown_option(HelpTopic::Add, &path)),
        Some(path) => PathBuf::from(path),
        None => PathBuf::from("."),
    };
    if let Some(extra) = args.next() {
        if extra.starts_with('-') {
            return Err(unknown_option(HelpTopic::Add, &extra));
        }
        return Err(usage_error(HelpTopic::Add, "add accepts at most one path"));
    }
    Ok(Command::Add { path })
}

fn parse_profile(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Profile));
    }
    let mut args = args.into_iter();
    let subcommand = args.next().unwrap_or_else(|| "list".into());
    let command = match subcommand.as_str() {
        "list" => {
            require_no_args(args.collect(), "profile list", HelpTopic::Profile)?;
            ProfileCommand::List
        }
        "create" => {
            let name = args
                .next()
                .ok_or_else(|| usage_error(HelpTopic::Profile, "profile create requires NAME"))?;
            if name.starts_with('-') {
                return Err(unknown_option(HelpTopic::Profile, &name));
            }
            let mut copy_current = true;
            for arg in args {
                match arg.as_str() {
                    "--empty" => copy_current = false,
                    _ => return Err(unknown_option(HelpTopic::Profile, &arg)),
                }
            }
            ProfileCommand::Create { name, copy_current }
        }
        "switch" => {
            let id = args
                .next()
                .ok_or_else(|| usage_error(HelpTopic::Profile, "profile switch requires ID"))?;
            let profile_id = parse_id_arg(&id, "profile", HelpTopic::Profile)?;
            let mut stop_running = false;
            for arg in args {
                match arg.as_str() {
                    "--stop-running" => stop_running = true,
                    _ => return Err(unknown_option(HelpTopic::Profile, &arg)),
                }
            }
            ProfileCommand::Switch {
                profile_id,
                stop_running,
            }
        }
        "export" => {
            let id = args.next().ok_or_else(|| {
                usage_error(HelpTopic::Profile, "profile export requires ID and PATH")
            })?;
            let profile_id = parse_id_arg(&id, "profile", HelpTopic::Profile)?;
            let path = args
                .next()
                .ok_or_else(|| usage_error(HelpTopic::Profile, "profile export requires PATH"))?;
            require_no_args(args.collect(), "profile export", HelpTopic::Profile)?;
            ProfileCommand::Export {
                profile_id,
                path: PathBuf::from(path),
            }
        }
        "import" => {
            let path = args
                .next()
                .ok_or_else(|| usage_error(HelpTopic::Profile, "profile import requires PATH"))?;
            let mut name = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--name" if name.is_none() => {
                        name = Some(next_value(&mut args, &arg, HelpTopic::Profile)?);
                    }
                    _ => return Err(unknown_option(HelpTopic::Profile, &arg)),
                }
            }
            ProfileCommand::Import {
                path: PathBuf::from(path),
                name,
            }
        }
        "delete" => {
            let id = args
                .next()
                .ok_or_else(|| usage_error(HelpTopic::Profile, "profile delete requires ID"))?;
            let profile_id = parse_id_arg(&id, "profile", HelpTopic::Profile)?;
            require_no_args(args.collect(), "profile delete", HelpTopic::Profile)?;
            ProfileCommand::Delete { profile_id }
        }
        _ if subcommand.starts_with('-') => {
            return Err(unknown_option(HelpTopic::Profile, &subcommand));
        }
        _ => {
            return Err(usage_error(
                HelpTopic::Profile,
                format!("unknown profile command {subcommand:?}"),
            ));
        }
    };
    Ok(Command::Profile(command))
}

fn parse_project_remove(args: Vec<String>) -> Result<Command> {
    parse_removal(args, HelpTopic::Project)
}

fn parse_worktree(args: Vec<String>) -> Result<Command> {
    parse_removal(args, HelpTopic::Worktree)
}

fn parse_removal(args: Vec<String>, topic: HelpTopic) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(topic));
    }
    let command_name = topic.command().expect("removal command help topic");
    let mut args = args.into_iter();
    let subcommand = args
        .next()
        .ok_or_else(|| usage_error(topic, format!("{command_name} requires the remove command")))?;
    if subcommand != "remove" {
        if subcommand.starts_with('-') {
            return Err(unknown_option(topic, &subcommand));
        }
        return Err(usage_error(
            topic,
            format!("unknown {command_name} command {subcommand:?}"),
        ));
    }

    let mut project_id = None;
    let mut delete_from_disk = false;
    let mut stop_running = false;
    let mut force_dirty = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" if project_id.is_none() => {
                project_id = Some(parse_id_arg(
                    &next_value(&mut args, &arg, topic)?,
                    "project",
                    topic,
                )?);
            }
            "--project" => {
                return Err(usage_error(topic, "--project may only be specified once"));
            }
            "--delete-local" if !delete_from_disk => delete_from_disk = true,
            "--delete-local" => {
                return Err(usage_error(
                    topic,
                    "--delete-local may only be specified once",
                ));
            }
            "--stop-running" if !stop_running => stop_running = true,
            "--stop-running" => {
                return Err(usage_error(
                    topic,
                    "--stop-running may only be specified once",
                ));
            }
            "--force" if !force_dirty => force_dirty = true,
            "--force" => {
                return Err(usage_error(topic, "--force may only be specified once"));
            }
            _ => return Err(unknown_option(topic, &arg)),
        }
    }
    if force_dirty && !delete_from_disk {
        return Err(usage_error(topic, "--force requires --delete-local"));
    }
    let command = WorktreeCommand::Remove {
        project_id,
        delete_from_disk,
        stop_running,
        force_dirty,
    };
    Ok(if topic == HelpTopic::Project {
        Command::Project(command)
    } else {
        Command::Worktree(command)
    })
}

fn parse_project_action(args: Vec<String>, start: bool) -> Result<Command> {
    let topic = if start {
        HelpTopic::Up
    } else {
        HelpTopic::Down
    };
    if help_requested(&args) {
        return Ok(Command::Help(topic));
    }
    let mut args = args.into_iter();
    let mut project_id = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" if project_id.is_none() => {
                project_id = Some(parse_id_arg(
                    &next_value(&mut args, &arg, topic)?,
                    "project",
                    topic,
                )?);
            }
            "--project" => {
                return Err(usage_error(topic, "--project may only be specified once"));
            }
            _ => return Err(unknown_option(topic, &arg)),
        }
    }
    Ok(if start {
        Command::Up { project_id }
    } else {
        Command::Down { project_id }
    })
}

fn parse_mcp_setup(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::McpSetup));
    }
    let mut args = args.into_iter();
    let mut run = false;
    let mut client = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--run" if !run => run = true,
            "--run" => {
                return Err(usage_error(
                    HelpTopic::McpSetup,
                    "--run may only be specified once",
                ));
            }
            "--client" => {
                if client.is_some() {
                    return Err(usage_error(
                        HelpTopic::McpSetup,
                        "--client may only be specified once",
                    ));
                }
                let value = next_value(&mut args, &arg, HelpTopic::McpSetup)?;
                client = Some(McpClient::parse(&value).ok_or_else(|| {
                    usage_error(
                        HelpTopic::McpSetup,
                        format!(
                            "unknown MCP client {value:?}; expected claude, codex, gemini, opencode, or generic"
                        ),
                    )
                })?);
            }
            _ => return Err(unknown_option(HelpTopic::McpSetup, &arg)),
        }
    }
    Ok(Command::McpSetup { run, client })
}

fn require_no_args(args: Vec<String>, command: &str, topic: HelpTopic) -> Result<()> {
    if let Some(option) = args.iter().find(|arg| arg.starts_with('-')) {
        return Err(unknown_option(topic, option));
    }
    if !args.is_empty() {
        return Err(usage_error(
            topic,
            format!("{command} does not accept arguments"),
        ));
    }
    Ok(())
}

fn parse_run(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Run));
    }
    let mut args = args.into_iter();
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
                if project_id.is_some() {
                    return Err(usage_error(
                        HelpTopic::Run,
                        "--project may only be specified once",
                    ));
                }
                project_id = Some(parse_id_arg(
                    &next_value(&mut args, &arg, HelpTopic::Run)?,
                    "project",
                    HelpTopic::Run,
                )?);
            }
            "--name" if command.is_empty() => {
                if name.is_some() {
                    return Err(usage_error(
                        HelpTopic::Run,
                        "--name may only be specified once",
                    ));
                }
                name = Some(next_value(&mut args, &arg, HelpTopic::Run)?);
            }
            "--cwd" if command.is_empty() => {
                if cwd.is_some() {
                    return Err(usage_error(
                        HelpTopic::Run,
                        "--cwd may only be specified once",
                    ));
                }
                cwd = Some(PathBuf::from(next_value(&mut args, &arg, HelpTopic::Run)?));
            }
            _ if arg.starts_with('-') => return Err(unknown_option(HelpTopic::Run, &arg)),
            _ => {
                command.push(arg);
                command.extend(args);
                break;
            }
        }
    }
    if command.is_empty() {
        return Err(usage_error(HelpTopic::Run, "run requires a command"));
    }
    reject_leading_dash(&command[0], "COMMAND", HelpTopic::Run)?;
    Ok(Command::Run(RunOptions {
        project_id,
        name,
        cwd,
        command,
    }))
}

fn parse_agent(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Agent));
    }
    let mut args = args.into_iter();
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
            "--project" if project_id.is_none() => {
                project_id = Some(parse_id_arg(
                    &next_value(&mut args, &arg, HelpTopic::Agent)?,
                    "project",
                    HelpTopic::Agent,
                )?);
            }
            "--project" => {
                return Err(usage_error(
                    HelpTopic::Agent,
                    "--project may only be specified once",
                ));
            }
            "--tool" => {
                if agent_tool_id.is_some() {
                    return Err(usage_error(
                        HelpTopic::Agent,
                        "--tool may only be specified once",
                    ));
                }
                agent_tool_id = Some(parse_id_arg(
                    &next_value(&mut args, &arg, HelpTopic::Agent)?,
                    "agent tool",
                    HelpTopic::Agent,
                )?);
            }
            "--name" if name.is_none() => {
                name = Some(next_value(&mut args, &arg, HelpTopic::Agent)?);
            }
            "--name" => {
                return Err(usage_error(
                    HelpTopic::Agent,
                    "--name may only be specified once",
                ));
            }
            _ => return Err(unknown_option(HelpTopic::Agent, &arg)),
        }
    }
    Ok(Command::Agent(AgentOptions {
        project_id,
        agent_tool_id: agent_tool_id
            .ok_or_else(|| usage_error(HelpTopic::Agent, "agent requires --tool ID"))?,
        name,
        extra_args,
    }))
}

fn parse_ps(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Ps));
    }
    let mut args = args.into_iter();
    let mut project_id = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" if project_id.is_none() => {
                project_id = Some(parse_id_arg(
                    &next_value(&mut args, &arg, HelpTopic::Ps)?,
                    "project",
                    HelpTopic::Ps,
                )?);
            }
            "--project" => {
                return Err(usage_error(
                    HelpTopic::Ps,
                    "--project may only be specified once",
                ));
            }
            _ => return Err(unknown_option(HelpTopic::Ps, &arg)),
        }
    }
    Ok(Command::Ps { project_id })
}

fn parse_logs(args: Vec<String>) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(HelpTopic::Logs));
    }
    let mut args = args.into_iter();
    let mut follow = false;
    let mut process_id = None;
    for arg in args.by_ref() {
        match arg.as_str() {
            "--follow" | "-f" if !follow => follow = true,
            "--follow" | "-f" => {
                return Err(usage_error(
                    HelpTopic::Logs,
                    "--follow may only be specified once",
                ));
            }
            _ if arg.starts_with('-') => return Err(unknown_option(HelpTopic::Logs, &arg)),
            _ if process_id.is_none() => {
                process_id = Some(parse_id_arg(&arg, "process", HelpTopic::Logs)?);
            }
            _ => {
                return Err(usage_error(
                    HelpTopic::Logs,
                    "logs accepts exactly one process ID",
                ));
            }
        }
    }
    Ok(Command::Logs {
        process_id: process_id
            .ok_or_else(|| usage_error(HelpTopic::Logs, "logs requires a process ID"))?,
        follow,
    })
}

fn parse_process_id_command(args: Vec<String>, topic: HelpTopic) -> Result<Command> {
    if help_requested(&args) {
        return Ok(Command::Help(topic));
    }
    let command = topic.command().expect("process command help topic");
    let mut args = args.into_iter();
    let id = args
        .next()
        .ok_or_else(|| usage_error(topic, format!("{command} requires a process ID")))?;
    if id.starts_with('-') {
        return Err(unknown_option(topic, &id));
    }
    if let Some(extra) = args.next() {
        if extra.starts_with('-') {
            return Err(unknown_option(topic, &extra));
        }
        return Err(usage_error(
            topic,
            format!("{command} accepts exactly one process ID"),
        ));
    }
    let process_id = parse_id_arg(&id, "process", topic)?;
    Ok(match topic {
        HelpTopic::Attach => Command::Attach { process_id },
        HelpTopic::Stop => Command::Stop { process_id },
        _ => unreachable!("process ID command topic"),
    })
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    option: &str,
    topic: HelpTopic,
) -> Result<String> {
    let value = args
        .next()
        .ok_or_else(|| usage_error(topic, format!("{option} requires a value")))?;
    reject_leading_dash(&value, &format!("{option} value"), topic)?;
    Ok(value)
}

fn reject_leading_dash(value: &str, label: &str, topic: HelpTopic) -> Result<()> {
    if value.starts_with('-') {
        return Err(usage_error(
            topic,
            format!("{label} must not start with '-'"),
        ));
    }
    Ok(())
}

fn parse_id_arg(value: &str, kind: &str, topic: HelpTopic) -> Result<i64> {
    parse_id(value, kind)
        .map_err(|_| usage_error(topic, format!("{kind} ID must be a positive integer")))
}

fn unknown_option(topic: HelpTopic, option: &str) -> Box<dyn Error + Send + Sync> {
    usage_error(topic, format!("unknown option {option:?}"))
}

fn usage_error(topic: HelpTopic, message: impl fmt::Display) -> Box<dyn Error + Send + Sync> {
    let hint = topic
        .command()
        .map(|command| format!("wrk {command} --help"))
        .unwrap_or_else(|| "wrk --help".into());
    cli_error(format!("{message}\nTry `{hint}` for usage."))
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

#[derive(Debug, Deserialize)]
struct ProfileListResponse {
    profiles: Vec<Profile>,
}

#[derive(Debug, Deserialize)]
struct ProfileResponse {
    profile: Profile,
    #[serde(default)]
    stopped_processes: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct WorktreeRemovalResponse {
    path: String,
    branch: String,
    project_unregistered: bool,
    deleted_from_disk: bool,
    metadata_pruned: bool,
    branch_kept: bool,
}

async fn profile(client: &mut Client, command: ProfileCommand) -> Result<()> {
    match command {
        ProfileCommand::List => {
            let response: ProfileListResponse = client.rpc("profile.list", json!({})).await?;
            if response.profiles.is_empty() {
                println!("No profiles.");
            } else {
                for profile in response.profiles {
                    println!(
                        "{} {:>3}  {}  · {} project{} · {} agent preset{}",
                        if profile.active { "*" } else { " " },
                        profile.id,
                        profile.name,
                        profile.project_count,
                        if profile.project_count == 1 { "" } else { "s" },
                        profile.agent_tool_count,
                        if profile.agent_tool_count == 1 {
                            ""
                        } else {
                            "s"
                        },
                    );
                }
            }
        }
        ProfileCommand::Create { name, copy_current } => {
            let response: ProfileResponse = client
                .rpc(
                    "profile.create",
                    json!({ "name": name, "copy_current": copy_current }),
                )
                .await?;
            println!(
                "Created profile {} ({}){}",
                response.profile.id,
                response.profile.name,
                if copy_current {
                    " from current state"
                } else {
                    " empty"
                }
            );
        }
        ProfileCommand::Switch {
            profile_id,
            stop_running,
        } => {
            let response: ProfileResponse = client
                .rpc(
                    "profile.switch",
                    json!({
                        "profile_id": profile_id,
                        "confirm_stop_running": stop_running,
                    }),
                )
                .await?;
            println!(
                "Switched to {} ({})",
                response.profile.id, response.profile.name
            );
            if !response.stopped_processes.is_empty() {
                println!(
                    "Stopped {} running process(es).",
                    response.stopped_processes.len()
                );
            }
        }
        ProfileCommand::Export { profile_id, path } => {
            let _: Value = client
                .rpc(
                    "profile.export",
                    json!({ "profile_id": profile_id, "path": path.to_string_lossy() }),
                )
                .await?;
            println!("Exported profile {profile_id} to {}", path.display());
        }
        ProfileCommand::Import { path, name } => {
            let response: ProfileResponse = client
                .rpc(
                    "profile.import",
                    json!({ "path": path.to_string_lossy(), "name": name }),
                )
                .await?;
            println!(
                "Imported profile {} ({}) from {}",
                response.profile.id,
                response.profile.name,
                path.display()
            );
        }
        ProfileCommand::Delete { profile_id } => {
            let _: Value = client
                .rpc(
                    "profile.delete",
                    json!({ "profile_id": profile_id, "confirm_delete": true }),
                )
                .await?;
            println!("Deleted profile {profile_id}");
        }
    }
    Ok(())
}

async fn project_remove(client: &mut Client, command: WorktreeCommand) -> Result<()> {
    match command {
        WorktreeCommand::Remove {
            project_id,
            delete_from_disk,
            stop_running,
            force_dirty,
        } => {
            let cwd = canonical_directory(&env::current_dir()?)?;
            let project_id = resolve_project_id(client, project_id, &cwd).await?;
            let response: WorktreeRemovalResponse = client
                .rpc(
                    "projects.remove",
                    json!({
                        "project_id": project_id,
                        "confirm_remove": true,
                        "confirm_stop_running": stop_running,
                        "delete_from_disk": delete_from_disk,
                        "force_dirty": force_dirty,
                    }),
                )
                .await?;
            if !response.project_unregistered {
                return Err(cli_error(format!(
                    "project {project_id} was not registered in the active profile"
                )));
            }
            println!("Removed {} from Workman.", response.branch);
            if response.deleted_from_disk {
                println!("Deleted local project at {}.", response.path);
                if response.metadata_pruned {
                    println!("Pruned Git worktree metadata.");
                }
            } else {
                println!("Local project kept at {}.", response.path);
            }
            if response.deleted_from_disk && response.branch_kept {
                println!("Git branch {} was kept.", response.branch);
            }
        }
    }
    Ok(())
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

    let _raw_mode = RawModeGuard::enable()?;
    send_resize(client, process_id).await?;
    let mut resize = resize_events();
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
            Some(()) = resize.recv() => send_resize(client, process_id).await?,
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
    let size = terminal_size().unwrap_or(TerminalSize {
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
    let server_name = RuntimeIdentity::current().mcp_server_name();
    let args = [
        "mcp",
        "add",
        "--transport",
        "http",
        server_name,
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

async fn launch_app(
    data_dir: &Path,
    config: &Path,
    daemon: &Path,
    identity: RuntimeIdentity,
) -> Result<()> {
    let target = desktop_launch_target(identity).ok_or_else(|| {
        cli_error(format!(
            "could not find {}; run the installer again or set WORKMAN_DESKTOP",
            identity.app_bundle_name()
        ))
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
        DesktopLaunchTarget::Bundle(bundle) => {
            launch_macos_bundle(&bundle, data_dir, config, daemon)?
        }
        DesktopLaunchTarget::Executable(executable) => {
            #[cfg(target_os = "macos")]
            eprintln!(
                "note: Workman.app was not found; launching {} directly as a development fallback (Dock branding is unavailable)",
                executable.display()
            );
            let child = ProcessCommand::new(&executable)
                .env("WORKMAN_DATA_DIR", data_dir)
                .env("WORKMAN_CONFIG", config)
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

fn desktop_launch_target(identity: RuntimeIdentity) -> Option<DesktopLaunchTarget> {
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
        identity.app_bundle_name(),
    ) {
        return Some(DesktopLaunchTarget::Bundle(bundle));
    }

    [
        directory.join("workman-desktop"),
        directory
            .join("bundle/macos")
            .join(identity.app_bundle_name())
            .join("Contents/MacOS/workman-desktop"),
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
    bundle_name: &str,
) -> Option<PathBuf> {
    let directory = current.parent()?;
    let mut candidates = vec![system_applications.join(bundle_name)];
    if let Some(home) = home {
        candidates.push(home.join("Applications").join(bundle_name));
    }
    candidates.push(directory.join("bundle/macos").join(bundle_name));
    if let Some(package_root) = directory.parent() {
        candidates.push(package_root.join(bundle_name));
    }
    candidates.push(directory.join(bundle_name));
    candidates
        .into_iter()
        .find(|bundle| bundle.is_dir() && bundle.join("Contents/MacOS/workman-desktop").is_file())
}

#[cfg(target_os = "macos")]
fn launch_macos_bundle(bundle: &Path, data_dir: &Path, config: &Path, daemon: &Path) -> Result<()> {
    let open = Path::new("/usr/bin/open");
    let supports_env = ProcessCommand::new(open)
        .arg("-h")
        .output()
        .is_ok_and(|output| {
            open_help_supports_env(&output.stdout) || open_help_supports_env(&output.stderr)
        });
    let status = ProcessCommand::new(open)
        .args(macos_open_args(
            bundle,
            data_dir,
            config,
            daemon,
            supports_env,
        ))
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
    println!(
        "✓ Opened {} through LaunchServices",
        bundle
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Workman app")
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn launch_macos_bundle(
    _bundle: &Path,
    _data_dir: &Path,
    _config: &Path,
    _daemon: &Path,
) -> Result<()> {
    unreachable!("bundle launch targets are only resolved on macOS")
}

#[cfg(any(target_os = "macos", test))]
fn macos_open_args(
    bundle: &Path,
    data_dir: &Path,
    config: &Path,
    daemon: &Path,
    supports_env: bool,
) -> Vec<OsString> {
    let mut args = vec![OsString::from("-a"), bundle.as_os_str().to_owned()];
    if supports_env {
        args.extend([
            OsString::from("--env"),
            environment_assignment("WORKMAN_DATA_DIR", data_dir),
            OsString::from("--env"),
            environment_assignment("WORKMAN_CONFIG", config),
            OsString::from("--env"),
            environment_assignment("WORKMAN_DAEMON_BIN", daemon),
        ]);
    } else {
        args.extend([
            OsString::from("--args"),
            OsString::from("--workman-data-dir"),
            data_dir.as_os_str().to_owned(),
            OsString::from("--workman-config"),
            config.as_os_str().to_owned(),
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

fn daemon_executable(explicit: Option<PathBuf>, identity: RuntimeIdentity) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(path) = env::var_os("WORKMAN_DAEMON") {
        return PathBuf::from(path);
    }
    if let Ok(current) = env::current_exe()
        && let Some(sibling) = sibling_daemon_executable(&current, identity)
    {
        return sibling;
    }
    PathBuf::from(identity.daemon_binary_name())
}

fn sibling_daemon_executable(current: &Path, identity: RuntimeIdentity) -> Option<PathBuf> {
    if identity.is_dev() {
        let daemon = current.with_file_name(identity.daemon_binary_name());
        return daemon.is_file().then_some(daemon);
    }
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
        let install_target = match env::var_os("WORKMAN_UPDATE_INSTALL_DIR") {
            Some(path) => UpdateInstallTarget::binary_directory(PathBuf::from(path)),
            None => UpdateInstallTarget::discover(env::current_exe()?)?,
        };
        updater.install_target(&check, &install_target).await?
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

#[cfg(unix)]
struct RawModeGuard {
    fd: RawFd,
    original: libc::termios,
    enabled: bool,
}

#[cfg(unix)]
impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        let fd = libc::STDIN_FILENO;
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

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.enabled {
            // SAFETY: original came from tcgetattr for this same terminal descriptor.
            let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
        }
    }
}

/// Console raw mode for Windows: byte-stream VT input in, VT sequences out.
///
/// Handles are stored as integers so the guard stays `Send`; they are stable
/// process pseudo-handles owned by the console subsystem, not resources to close.
#[cfg(windows)]
struct RawModeGuard {
    input: isize,
    output: isize,
    original_input: u32,
    original_output: u32,
    restore_output: bool,
    enabled: bool,
}

#[cfg(windows)]
impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        use windows_sys::Win32::System::Console::{
            DISABLE_NEWLINE_AUTO_RETURN, ENABLE_ECHO_INPUT, ENABLE_EXTENDED_FLAGS,
            ENABLE_LINE_INPUT, ENABLE_MOUSE_INPUT, ENABLE_PROCESSED_INPUT, ENABLE_QUICK_EDIT_MODE,
            ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode,
            GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
        };

        // SAFETY: std handles are process-owned pseudo-handles and every mode value
        // is initialized by the console call that precedes its use.
        unsafe {
            let input = GetStdHandle(STD_INPUT_HANDLE);
            let mut original_input = 0_u32;
            if GetConsoleMode(input, &mut original_input) == 0 {
                // Redirected stdin is not a console; raw mode is unnecessary.
                return Ok(Self {
                    input: 0,
                    output: 0,
                    original_input: 0,
                    original_output: 0,
                    restore_output: false,
                    enabled: false,
                });
            }

            let raw_input = (original_input
                & !(ENABLE_ECHO_INPUT
                    | ENABLE_LINE_INPUT
                    | ENABLE_PROCESSED_INPUT
                    | ENABLE_MOUSE_INPUT
                    | ENABLE_QUICK_EDIT_MODE))
                | ENABLE_VIRTUAL_TERMINAL_INPUT
                | ENABLE_EXTENDED_FLAGS;
            if SetConsoleMode(input, raw_input) == 0 {
                return Err(io::Error::last_os_error());
            }

            let output = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut original_output = 0_u32;
            let restore_output = GetConsoleMode(output, &mut original_output) != 0
                && SetConsoleMode(
                    output,
                    original_output
                        | ENABLE_VIRTUAL_TERMINAL_PROCESSING
                        | DISABLE_NEWLINE_AUTO_RETURN,
                ) != 0;

            Ok(Self {
                input: input as isize,
                output: output as isize,
                original_input,
                original_output,
                restore_output,
                enabled: true,
            })
        }
    }
}

#[cfg(windows)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Console::SetConsoleMode;

        if self.enabled {
            // SAFETY: both modes came from GetConsoleMode on these same std handles.
            unsafe {
                let _ = SetConsoleMode(self.input as _, self.original_input);
                if self.restore_output {
                    let _ = SetConsoleMode(self.output as _, self.original_output);
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TerminalSize {
    rows: u16,
    cols: u16,
    pixel_width: u16,
    pixel_height: u16,
}

#[cfg(unix)]
fn terminal_size() -> io::Result<TerminalSize> {
    // SAFETY: winsize is a plain integer struct initialized before ioctl writes it.
    let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
    // SAFETY: TIOCGWINSZ writes a winsize to the valid pointer for standard output.
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(TerminalSize {
        rows: size.ws_row.max(1),
        cols: size.ws_col.max(1),
        pixel_width: size.ws_xpixel,
        pixel_height: size.ws_ypixel,
    })
}

#[cfg(windows)]
fn terminal_size() -> io::Result<TerminalSize> {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    // SAFETY: the std handle is process-owned and the buffer info is plain data
    // written by the call before any field is read.
    unsafe {
        let output = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut info = std::mem::zeroed::<CONSOLE_SCREEN_BUFFER_INFO>();
        if GetConsoleScreenBufferInfo(output, &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        let rows = i32::from(info.srWindow.Bottom) - i32::from(info.srWindow.Top) + 1;
        let cols = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
        Ok(TerminalSize {
            rows: u16::try_from(rows).unwrap_or(1).max(1),
            cols: u16::try_from(cols).unwrap_or(1).max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
    }
}

/// Deliver one tick for every resize of the local terminal.
///
/// Unix subscribes to SIGWINCH. Windows has no resize signal, so the console
/// size is polled and a tick is sent only when it changes. A closed or
/// unavailable source simply stops producing ticks; attach keeps running.
fn resize_events() -> tokio::sync::mpsc::Receiver<()> {
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    #[cfg(unix)]
    tokio::spawn(async move {
        let Ok(mut signals) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
        else {
            return;
        };
        while signals.recv().await.is_some() {
            if sender.send(()).await.is_err() {
                return;
            }
        }
    });
    #[cfg(windows)]
    tokio::spawn(async move {
        let mut last = terminal_size().ok();
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let current = terminal_size().ok();
            if current != last {
                last = current;
                if sender.send(()).await.is_err() {
                    return;
                }
            }
        }
    });
    receiver
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
    fn dev_identity_is_visible_and_updates_only_by_rebuild() {
        assert_eq!(
            version_text(RuntimeIdentity::Stable),
            concat!("workman ", env!("CARGO_PKG_VERSION"))
        );
        let dev_version = version_text(RuntimeIdentity::Dev);
        assert!(dev_version.starts_with(concat!(
            "workman-dev ",
            env!("CARGO_PKG_VERSION"),
            " (build "
        )));
        assert!(dev_version.ends_with(')'));

        let root_help = help_text_for(RuntimeIdentity::Dev, HelpTopic::Root);
        assert!(root_help.starts_with(concat!("wrk-dev ", env!("CARGO_PKG_VERSION"))));
        assert!(root_help.contains("Usage: wrk-dev "));
        let update_help = help_text_for(RuntimeIdentity::Dev, HelpTopic::Update);
        assert!(update_help.contains("scripts/dev-install.sh"));
        assert!(dev_update_notice().contains("stable Workman was not changed"));
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
            macos_app_bundle_from(
                &wrk,
                None,
                &root.path().join("Applications"),
                RuntimeIdentity::Stable.app_bundle_name(),
            ),
            Some(bundled)
        );
    }

    #[test]
    fn macos_bundle_resolution_prefers_system_applications() {
        let root = tempfile::tempdir().unwrap();
        let wrk = root.path().join("bin/wrk");
        let home = root.path().join("home");
        let system_applications = root.path().join("Applications");
        let user_bundle = home.join("Applications/Workman.app");
        let system_bundle = system_applications.join("Workman.app");
        create_test_app_bundle(&system_bundle);
        create_test_app_bundle(&user_bundle);

        assert_eq!(
            macos_app_bundle_from(
                &wrk,
                Some(&home),
                &system_applications,
                RuntimeIdentity::Stable.app_bundle_name(),
            ),
            Some(system_bundle)
        );
    }

    #[test]
    fn dev_bundle_resolution_never_selects_the_stable_app() {
        let root = tempfile::tempdir().unwrap();
        let wrk_dev = root.path().join("bin/wrk-dev");
        let home = root.path().join("home");
        let system_applications = root.path().join("Applications");
        create_test_app_bundle(&system_applications.join("Workman.app"));
        let dev_bundle = home.join("Applications/Workman Dev.app");
        create_test_app_bundle(&dev_bundle);

        assert_eq!(
            macos_app_bundle_from(
                &wrk_dev,
                Some(&home),
                &system_applications,
                RuntimeIdentity::Dev.app_bundle_name(),
            ),
            Some(dev_bundle)
        );
    }

    #[test]
    fn macos_open_uses_launchservices_env_without_requesting_a_new_instance() {
        let bundle = Path::new("/tmp/Workman.app");
        let args = macos_open_args(
            bundle,
            Path::new("/tmp/workman data"),
            Path::new("/tmp/workman config.yml"),
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
                OsString::from("WORKMAN_CONFIG=/tmp/workman config.yml"),
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
            Path::new("/tmp/config.yml"),
            Path::new("/tmp/workmand"),
            false,
        );
        assert!(args.contains(&OsString::from("--args")));
        assert!(args.contains(&OsString::from("--workman-data-dir")));
        assert!(args.contains(&OsString::from("--workman-config")));
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
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.8");

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

        let cli = Cli::parse(["wrk", "worktree", "remove"].map(OsString::from)).unwrap();
        assert!(matches!(
            cli.command,
            Command::Worktree(WorktreeCommand::Remove {
                project_id: None,
                delete_from_disk: false,
                stop_running: false,
                force_dirty: false,
            })
        ));
        let cli = Cli::parse(
            [
                "wrk",
                "worktree",
                "remove",
                "--project",
                "8",
                "--delete-local",
                "--stop-running",
                "--force",
            ]
            .map(OsString::from),
        )
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Worktree(WorktreeCommand::Remove {
                project_id: Some(8),
                delete_from_disk: true,
                stop_running: true,
                force_dirty: true,
            })
        ));
        assert!(Cli::parse(["wrk", "worktree", "remove", "--force"].map(OsString::from)).is_err());
        assert!(
            Cli::parse(["wrk", "worktree", "remove", "--confirm", "main"].map(OsString::from))
                .is_err()
        );

        let cli = Cli::parse(
            [
                "wrk",
                "project",
                "remove",
                "--project",
                "9",
                "--delete-local",
                "--stop-running",
                "--force",
            ]
            .map(OsString::from),
        )
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Project(WorktreeCommand::Remove {
                project_id: Some(9),
                delete_from_disk: true,
                stop_running: true,
                force_dirty: true,
            })
        ));
        assert!(Cli::parse(["wrk", "project", "remove", "--force"].map(OsString::from)).is_err());

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

        let cli = Cli::parse(["wrk", "profile", "create", "Demo", "--empty"].map(OsString::from))
            .unwrap();
        assert!(matches!(
            cli.command,
            Command::Profile(ProfileCommand::Create {
                ref name,
                copy_current: false,
            }) if name == "Demo"
        ));
        let cli =
            Cli::parse(["wrk", "profile", "switch", "3", "--stop-running"].map(OsString::from))
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Profile(ProfileCommand::Switch {
                profile_id: 3,
                stop_running: true,
            })
        ));
        let cli = Cli::parse(
            [
                "wrk",
                "profile",
                "import",
                "/tmp/demo.json",
                "--name",
                "Imported",
            ]
            .map(OsString::from),
        )
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Profile(ProfileCommand::Import { ref name, .. })
                if name.as_deref() == Some("Imported")
        ));
    }

    #[test]
    fn root_and_every_subcommand_have_safe_help() {
        for args in [
            vec!["wrk", "--help"],
            vec!["wrk", "-h"],
            vec!["wrk", "help"],
            vec!["wrk", "--data-dir", "--help"],
            vec!["wrk", "--unknown", "--help"],
        ] {
            let cli = Cli::parse(args.into_iter().map(OsString::from)).unwrap();
            assert!(matches!(cli.command, Command::Help(HelpTopic::Root)));
        }

        assert!(ROOT_HELP.starts_with(concat!("wrk ", env!("CARGO_PKG_VERSION"))));
        assert!(ROOT_HELP.contains("Workspace\n"));
        assert!(ROOT_HELP.contains("Processes\n"));
        assert!(ROOT_HELP.contains("Worktrees\n"));
        assert!(ROOT_HELP.contains("WORKMAN_DATA_DIR=PATH"));
        assert!(ROOT_HELP.contains("WORKMAN_REQUIRE_EXPLICIT_DAEMON=1"));
        assert!(ROOT_HELP.contains("Docs: https://github.com/adrenallen/workman"));

        for (command, topic) in HelpTopic::SUBCOMMANDS {
            for args in [
                vec!["wrk", command, "--help"],
                vec!["wrk", command, "-h"],
                vec!["wrk", "help", command],
            ] {
                let cli = Cli::parse(args.into_iter().map(OsString::from)).unwrap();
                assert!(
                    matches!(cli.command, Command::Help(actual) if actual == topic),
                    "wrong help topic for {command}"
                );
            }
            let help = help_text(topic);
            assert!(help.contains(&format!("Usage: wrk {command}")), "{help}");
            assert!(help.ends_with('\n'), "{command} help needs a final newline");
        }

        let cli = Cli::parse(
            ["wrk", "run", "--unknown", "--help"]
                .into_iter()
                .map(OsString::from),
        )
        .unwrap();
        assert!(matches!(cli.command, Command::Help(HelpTopic::Run)));
    }

    #[test]
    fn explicit_daemon_guard_requires_a_data_directory_boundary() {
        let guard = Some(OsStr::new("1"));
        let data_dir = Path::new("/tmp/workman-todo405");

        let error = require_explicit_daemon_target(None, None, guard).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("blocked implicit default-daemon access")
        );
        assert!(error.to_string().contains("--data-dir PATH"));
        assert!(error.to_string().contains("WORKMAN_DATA_DIR=PATH"));
        assert!(require_explicit_daemon_target(Some(data_dir), None, guard).is_ok());
        assert!(
            require_explicit_daemon_target(None, Some(OsStr::new("/tmp/workman-todo405")), guard,)
                .is_ok()
        );
        assert!(require_explicit_daemon_target(None, None, Some(OsStr::new("true"))).is_ok());
        assert!(require_explicit_daemon_target(None, None, None).is_ok());
    }

    #[test]
    fn data_dir_flag_takes_precedence_over_environment() {
        assert_eq!(
            resolve_data_dir(
                Some(PathBuf::from("/tmp/workman-flag")),
                Some(OsString::from("/tmp/workman-environment")),
            ),
            Path::new("/tmp/workman-flag")
        );
        assert_eq!(
            resolve_data_dir(None, Some(OsString::from("/tmp/workman-environment"))),
            Path::new("/tmp/workman-environment")
        );
    }

    #[tokio::test]
    async fn guarded_cli_invocation_stops_before_default_daemon_discovery() {
        let error = run(
            ["wrk", "ps"].map(OsString::from),
            None,
            Some(OsString::from("1")),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains(REQUIRE_EXPLICIT_DAEMON_ENV));
        assert!(
            error
                .to_string()
                .contains("blocked implicit default-daemon access")
        );
    }

    #[test]
    fn unknown_flags_and_leading_dash_values_fail_closed() {
        let unknown_flag_cases: &[&[&str]] = &[
            &["wrk", "--unknown"],
            &["wrk", "add", "--unknown"],
            &["wrk", "add", ".", "--unknown"],
            &["wrk", "up", "--unknown"],
            &["wrk", "down", "--unknown"],
            &["wrk", "app", "--unknown"],
            &["wrk", "app", "extra", "--unknown"],
            &["wrk", "update", "--unknown"],
            &["wrk", "mcp-setup", "--unknown"],
            &["wrk", "run", "--unknown"],
            &["wrk", "agent", "--unknown"],
            &["wrk", "ps", "--unknown"],
            &["wrk", "logs", "--unknown"],
            &["wrk", "attach", "--unknown"],
            &["wrk", "attach", "1", "--unknown"],
            &["wrk", "stop", "--unknown"],
            &["wrk", "help", "--unknown"],
        ];
        for args in unknown_flag_cases {
            let error = Cli::parse(args.iter().copied().map(OsString::from)).unwrap_err();
            let error = error.to_string();
            assert!(error.contains("unknown option"), "{args:?}: {error}");
            assert!(error.contains("for usage"), "{args:?}: {error}");
        }

        let leading_value_cases: &[&[&str]] = &[
            &["wrk", "--data-dir", "--unsafe"],
            &["wrk", "run", "--name", "--unsafe", "echo"],
            &["wrk", "run", "--cwd", "--unsafe", "echo"],
            &["wrk", "run", "--", "--unsafe"],
            &["wrk", "agent", "--tool", "--unsafe"],
            &["wrk", "agent", "--tool", "1", "--name", "--unsafe"],
        ];
        for args in leading_value_cases {
            let error = Cli::parse(args.iter().copied().map(OsString::from)).unwrap_err();
            let error = error.to_string();
            assert!(
                error.contains("must not start with '-'"),
                "{args:?}: {error}"
            );
            assert!(error.contains("for usage"), "{args:?}: {error}");
        }

        let cli = Cli::parse(
            ["wrk", "run", "--", "npm", "--help"]
                .into_iter()
                .map(OsString::from),
        )
        .unwrap();
        let Command::Run(run) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(run.command, ["npm", "--help"]);
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
        assert_eq!(
            sibling_daemon_executable(&cli, RuntimeIdentity::Stable),
            Some(legacy)
        );

        let canonical = directory.path().join("workmand");
        std::fs::write(&canonical, "workmand").unwrap();
        assert_eq!(
            sibling_daemon_executable(&cli, RuntimeIdentity::Stable),
            Some(canonical)
        );
    }

    #[test]
    fn dev_daemon_sibling_never_falls_back_to_stable() {
        let directory = tempfile::tempdir().unwrap();
        let cli = directory.path().join("wrk-dev");
        std::fs::write(directory.path().join("workmand"), "stable daemon").unwrap();
        assert_eq!(sibling_daemon_executable(&cli, RuntimeIdentity::Dev), None);

        let dev = directory.path().join("workmand-dev");
        std::fs::write(&dev, "dev daemon").unwrap();
        assert_eq!(
            sibling_daemon_executable(&cli, RuntimeIdentity::Dev),
            Some(dev)
        );
    }
}
