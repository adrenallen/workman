//! Process-tree listener discovery and readiness waiting.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    error::Error,
    fmt, io,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use awm_core::{Process, ProcessId, ProcessStatus, ProjectId};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::time::{Instant, sleep};

use crate::{RegistryError, SharedProcessRegistry};

/// Default maximum wait used by the WebSocket and future MCP adapters.
pub const DEFAULT_PORT_WAIT: Duration = Duration::from_secs(30);
/// Upper bound accepted by remote adapters for one readiness wait.
pub const MAX_PORT_WAIT: Duration = Duration::from_secs(300);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Readiness derived from process state and detected TCP listeners.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    Stopped,
    Waiting,
    Ready,
}

/// Transport associated with a detected socket.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceProtocol {
    Tcp,
    Udp,
}

/// One listener attributed to an awm process or one of its descendants.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BoundListener {
    pub listener_pid: u32,
    pub address: String,
    pub port: u16,
    pub protocol: ServiceProtocol,
    pub url: String,
}

/// Ephemeral service/readiness view for one awm process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Service {
    pub process_id: ProcessId,
    pub name: String,
    pub process_status: ProcessStatus,
    pub root_pid: Option<u32>,
    pub readiness: ReadinessState,
    pub ports: Vec<u16>,
    pub urls: Vec<String>,
    pub listeners: Vec<BoundListener>,
}

/// Solo-compatible result shape for a bounded port wait.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WaitForBoundPortResult {
    pub ready: bool,
    pub timed_out: bool,
    pub process_id: ProcessId,
    pub services: Vec<Service>,
}

/// Platform-neutral listener record returned by a detector backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectedListener {
    pub listener_pid: u32,
    pub address: SocketAddr,
    pub protocol: ServiceProtocol,
}

/// Backend boundary kept independent from WS/MCP adapters and process persistence.
pub trait PortDetector: Send + Sync {
    fn listeners_for_roots(
        &self,
        root_pids: &[u32],
    ) -> io::Result<HashMap<u32, Vec<DetectedListener>>>;
}

/// Native detector using `listeners` for socket ownership and `sysinfo` for process ancestry.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemPortDetector;

impl PortDetector for SystemPortDetector {
    fn listeners_for_roots(
        &self,
        root_pids: &[u32],
    ) -> io::Result<HashMap<u32, Vec<DetectedListener>>> {
        let mut detected = root_pids
            .iter()
            .copied()
            .map(|pid| (pid, Vec::new()))
            .collect::<HashMap<_, _>>();
        if root_pids.is_empty() {
            return Ok(detected);
        }

        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let parents = system
            .processes()
            .values()
            .filter_map(|process| {
                process
                    .parent()
                    .map(|parent| (process.pid().as_u32(), parent.as_u32()))
            })
            .collect::<HashMap<_, _>>();
        let trees = root_pids
            .iter()
            .copied()
            .map(|root| (root, descendant_pids(root, &parents)))
            .collect::<HashMap<_, _>>();
        let listeners =
            listeners::get_all().map_err(|error| io::Error::other(error.to_string()))?;

        for listener in listeners {
            if listener.protocol != listeners::Protocol::TCP
                || listener.state != listeners::SocketState::Listen
                || listener.socket.port() == 0
            {
                continue;
            }
            for (root, tree) in &trees {
                if tree.contains(&listener.process.pid) {
                    detected.entry(*root).or_default().push(DetectedListener {
                        listener_pid: listener.process.pid,
                        address: listener.socket,
                        protocol: ServiceProtocol::Tcp,
                    });
                }
            }
        }
        for listeners in detected.values_mut() {
            listeners.sort_by_key(|listener| {
                (
                    listener.address.port(),
                    listener.listener_pid,
                    listener.address,
                )
            });
            listeners.dedup();
        }
        Ok(detected)
    }
}

/// Errors shared by readiness callers over WS and MCP.
#[derive(Debug)]
pub enum ReadinessError {
    Registry(RegistryError),
    Detection(io::Error),
    Worker(String),
}

impl ReadinessError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Registry(error) => error.code(),
            Self::Detection(_) => "port_detection_error",
            Self::Worker(_) => "readiness_worker_error",
        }
    }
}

impl fmt::Display for ReadinessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(error) => error.fmt(formatter),
            Self::Detection(error) => {
                write!(formatter, "could not inspect listening ports: {error}")
            }
            Self::Worker(error) => write!(formatter, "readiness worker failed: {error}"),
        }
    }
}

impl Error for ReadinessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Registry(error) => Some(error),
            Self::Detection(error) => Some(error),
            Self::Worker(_) => None,
        }
    }
}

impl From<RegistryError> for ReadinessError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

/// Stateless readiness facade reusable by WS and MCP adapters.
#[derive(Clone)]
pub struct ReadinessService {
    detector: Arc<dyn PortDetector>,
    poll_interval: Duration,
}

impl Default for ReadinessService {
    fn default() -> Self {
        Self::new(Arc::new(SystemPortDetector))
    }
}

impl fmt::Debug for ReadinessService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadinessService")
            .field("poll_interval", &self.poll_interval)
            .finish_non_exhaustive()
    }
}

impl ReadinessService {
    pub fn new(detector: Arc<dyn PortDetector>) -> Self {
        Self {
            detector,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    /// List active project processes, including `waiting` entries with no listener yet.
    pub async fn services_list(
        &self,
        registry: &SharedProcessRegistry,
        project_id: Option<ProjectId>,
    ) -> Result<Vec<Service>, ReadinessError> {
        let processes = registry
            .lock()
            .await
            .list(project_id)?
            .into_iter()
            .filter(|process| is_active(process.status))
            .collect::<Vec<_>>();
        self.detect(processes).await
    }

    /// Return current listeners and readiness for one process.
    pub async fn get_process_ports(
        &self,
        registry: &SharedProcessRegistry,
        process_id: ProcessId,
    ) -> Result<Service, ReadinessError> {
        let process = registry.lock().await.get(process_id)?;
        self.detect(vec![process])
            .await?
            .pop()
            .ok_or_else(|| ReadinessError::Worker("detector omitted process result".into()))
    }

    /// Wait until any TCP listener appears in the process tree or the deadline expires.
    pub async fn wait_for_bound_port(
        &self,
        registry: &SharedProcessRegistry,
        process_id: ProcessId,
        timeout: Duration,
    ) -> Result<WaitForBoundPortResult, ReadinessError> {
        let deadline = Instant::now() + timeout;
        loop {
            let service = self.get_process_ports(registry, process_id).await?;
            if service.readiness == ReadinessState::Ready {
                return Ok(WaitForBoundPortResult {
                    ready: true,
                    timed_out: false,
                    process_id,
                    services: vec![service],
                });
            }

            let now = Instant::now();
            if now >= deadline {
                return Ok(WaitForBoundPortResult {
                    ready: false,
                    timed_out: true,
                    process_id,
                    services: Vec::new(),
                });
            }
            sleep(self.poll_interval.min(deadline - now)).await;
        }
    }

    async fn detect(&self, processes: Vec<Process>) -> Result<Vec<Service>, ReadinessError> {
        let detector = self.detector.clone();
        tokio::task::spawn_blocking(move || services_from_processes(detector.as_ref(), processes))
            .await
            .map_err(|error| ReadinessError::Worker(error.to_string()))?
            .map_err(ReadinessError::Detection)
    }
}

fn services_from_processes(
    detector: &dyn PortDetector,
    processes: Vec<Process>,
) -> io::Result<Vec<Service>> {
    let root_pids = processes
        .iter()
        .filter(|process| is_active(process.status))
        .filter_map(|process| process.pid)
        .collect::<Vec<_>>();
    let mut detected = detector.listeners_for_roots(&root_pids)?;
    Ok(processes
        .into_iter()
        .map(|process| {
            let listeners = process
                .pid
                .and_then(|pid| detected.remove(&pid))
                .unwrap_or_default();
            service_from_process(process, listeners)
        })
        .collect())
}

fn service_from_process(process: Process, detected: Vec<DetectedListener>) -> Service {
    let mut listeners = detected
        .into_iter()
        .map(|listener| BoundListener {
            listener_pid: listener.listener_pid,
            address: listener.address.to_string(),
            port: listener.address.port(),
            protocol: listener.protocol,
            url: localhost_url(listener.address.port()),
        })
        .collect::<Vec<_>>();
    listeners.sort_by_key(|listener| {
        (
            listener.port,
            listener.listener_pid,
            listener.address.clone(),
        )
    });
    listeners.dedup();
    let ports = listeners
        .iter()
        .map(|listener| listener.port)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let urls = ports
        .iter()
        .map(|port| localhost_url(*port))
        .collect::<Vec<_>>();
    let readiness = if !is_active(process.status) {
        ReadinessState::Stopped
    } else if listeners.is_empty() {
        ReadinessState::Waiting
    } else {
        ReadinessState::Ready
    };
    Service {
        process_id: process.id,
        name: process.name,
        process_status: process.status,
        root_pid: process.pid,
        readiness,
        ports,
        urls,
        listeners,
    }
}

fn descendant_pids(root: u32, parents: &HashMap<u32, u32>) -> HashSet<u32> {
    let mut descendants = HashSet::from([root]);
    loop {
        let before = descendants.len();
        for (pid, parent) in parents {
            if descendants.contains(parent) {
                descendants.insert(*pid);
            }
        }
        if descendants.len() == before {
            return descendants;
        }
    }
}

fn localhost_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

fn is_active(status: ProcessStatus) -> bool {
    matches!(status, ProcessStatus::Starting | ProcessStatus::Running)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendant_tree_includes_all_generations() {
        let parents = HashMap::from([(11, 10), (12, 11), (20, 1)]);
        assert_eq!(descendant_pids(10, &parents), HashSet::from([10, 11, 12]));
    }
}
