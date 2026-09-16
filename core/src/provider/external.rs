use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

/// Identity of one plugin as described by an external host's `list_plugins`.
/// The host owns its own name/icon/ready hint; the core just relays them.
#[derive(Clone)]
pub struct HostMeta {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub ready: String,
}

/// A plugin whose results come from an external JSON-RPC subprocess declared
/// in `plugins.toml` via `command`. The core is a generic client: it spawns
/// `command`, relays `search`, and discovers the plugin's identity from the
/// host's `list_plugins` response. It has no compiled-in knowledge of the
/// plugin — the same binary can serve any number of ids.
pub struct External {
    meta: Meta,
    command: String,
}

/// The registry is built once per process, so these small strings live exactly
/// the process lifetime that `Meta`'s `&'static str` requires. Interning keeps
/// that true across `plugins.toml` reloads: re-registering the same identity
/// reuses the string instead of leaking a second copy.
fn leak(s: String) -> &'static str {
    static POOL: LazyLock<StdMutex<HashSet<&'static str>>> =
        LazyLock::new(|| StdMutex::new(HashSet::new()));

    let mut pool = POOL.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(existing) = pool.get(s.as_str()) {
        return existing;
    }
    let leaked: &'static str = Box::leak(s.into_boxed_str());
    pool.insert(leaked);
    leaked
}

impl External {
    /// Build from the configured id + host command + host-discovered identity.
    /// `None` identity (host missing or not self-describing) degrades to the
    /// id as display name; search still relays and just yields no results.
    pub fn new(id: &str, command: String, discovered: Option<HostMeta>) -> Self {
        let (name, icon, ready) = match discovered {
            Some(m) => (m.name, m.icon, m.ready),
            None => (
                id.to_string(),
                String::new(),
                format!("External plugin via {command}"),
            ),
        };
        // The host may name its identity with a `papirus:` spec; the UI only
        // renders absolute paths (`file://` + icon), so resolve before the
        // string leaks into `Meta`.
        let icon = if icon.starts_with("papirus:") {
            find_icon_path(&icon).unwrap_or_default()
        } else {
            icon
        };
        Self {
            meta: Meta {
                id: leak(id.to_string()),
                name: leak(name),
                icon: leak(icon),
                ready: leak(ready),
                keyword: leak(id.to_string()),
            },
            command,
        }
    }
}

impl Plugin for External {
    fn meta(&self) -> &Meta {
        &self.meta
    }

    fn search(
        &self,
        query: &str,
        _full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let command = self.command.clone();
        let plugin = self.meta.id.to_string();
        let query = query.to_string();
        let icon = self.meta.icon.to_string();
        Box::pin(async move { query_external(&command, &plugin, &query, &icon).await })
    }

    fn default_view(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<ResultItem>>>> + Send + '_>> {
        let command = self.command.clone();
        let plugin = self.meta.id.to_string();
        let icon = self.meta.icon.to_string();
        Box::pin(async move { query_default(&command, &plugin, &icon).await })
    }

    fn forget(&self, on_click: &str) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        let command = self.command.clone();
        let on_click = on_click.to_string();
        Box::pin(async move { forget_external(&command, &on_click).await })
    }
}

/// Bound on one host round trip: a wedged host must not hold the session (and
/// every later search against it) forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Keep a host this long after its last request: one interpreter serves a
/// query's keystrokes, but nothing stays resident once the launcher goes quiet.
const SESSION_IDLE: Duration = Duration::from_secs(60);
/// Bound on one-shot discovery: a host that never exits must not hang the
/// registry build.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the idle reaper runs while at least one host is alive.
const REAP_INTERVAL: Duration = Duration::from_secs(20);

/// One long-lived host process: the plugin framework loops on stdin until EOF
/// (docs/en/jsonrpc.md), so one interpreter answers every request instead of
/// being spawned per keystroke.
struct Session {
    /// Dropped (and killed, via `kill_on_drop`) with the session.
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

enum ExchangeError {
    /// Child gone / pipe broken (also a one-shot host on its second request):
    /// respawning may recover.
    Dead,
    /// No matching response in time: the host is wedged, do not retry.
    Timeout,
}

struct Host {
    session: Mutex<Session>,
    last_used: StdMutex<Instant>,
}

static HOSTS: LazyLock<StdMutex<HashMap<String, Arc<Host>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));
/// Responses are matched by id: an aborted search can leave a stale one in the
/// pipe.
static NEXT_HOST_REQUEST: AtomicU64 = AtomicU64::new(1);

fn host_request_id() -> u64 {
    NEXT_HOST_REQUEST.fetch_add(1, Ordering::Relaxed)
}

fn lock_hosts() -> MutexGuard<'static, HashMap<String, Arc<Host>>> {
    HOSTS.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Session {
    fn spawn(command: &str) -> Option<Session> {
        let mut child = Command::new(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            // an aborted search must not leave the interpreter behind
            .kill_on_drop(true)
            .spawn()
            .ok()?; // command not found -> no host

        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        Some(Session {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// Write one request, read its response. Non-JSON lines (a host may log to
    /// stdout) and responses to a superseded request are skipped.
    async fn exchange(
        &mut self,
        request: &str,
        id: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, ExchangeError> {
        if self.stdin.write_all(request.as_bytes()).await.is_err()
            || self.stdin.flush().await.is_err()
        {
            return Err(ExchangeError::Dead);
        }

        let read = async {
            loop {
                let mut line = Vec::new();
                let read = self
                    .stdout
                    .read_until(b'\n', &mut line)
                    .await
                    .map_err(|_| ExchangeError::Dead)?;
                if read == 0 {
                    return Err(ExchangeError::Dead); // EOF: host exited
                }

                let text = String::from_utf8_lossy(&line);
                let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
                    continue;
                };
                if value.get("result").is_none() && value.get("error").is_none() {
                    continue;
                }
                if let Some(want) = id
                    && let Some(got) = value.get("id")
                    && got != want
                {
                    continue; // response to the request this one superseded
                }
                return Ok(value);
            }
        };

        match tokio::time::timeout(REQUEST_TIMEOUT, read).await {
            Ok(result) => result,
            Err(_) => Err(ExchangeError::Timeout),
        }
    }
}

/// The live session for `command`, spawned on first use: a plugin nobody
/// searches never costs a process.
fn session_for(command: &str) -> Option<Arc<Host>> {
    let mut hosts = lock_hosts();
    if let Some(host) = hosts.get(command) {
        return Some(host.clone());
    }

    let session = Session::spawn(command)?;
    let host = Arc::new(Host {
        session: Mutex::new(session),
        last_used: StdMutex::new(Instant::now()),
    });
    hosts.insert(command.to_string(), host.clone());
    drop(hosts);

    ensure_reaper();
    Some(host)
}

/// Drop this exact session (never a newer one for the same command).
fn drop_session(command: &str, host: &Arc<Host>) {
    let mut hosts = lock_hosts();
    let is_current = hosts
        .get(command)
        .is_some_and(|current| Arc::ptr_eq(current, host));
    if is_current {
        hosts.remove(command); // dropping the Arc drops (and kills) the child
    }
}

/// Kill hosts idle for [`SESSION_IDLE`]. Only sessions nobody else holds are
/// reaped, so an in-flight request is never killed under it.
fn reap_idle(now: Instant) {
    let expired: Vec<Arc<Host>> = {
        let hosts = lock_hosts();
        hosts
            .values()
            .filter(|host| {
                Arc::strong_count(host) == 1
                    && host
                        .last_used
                        .lock()
                        .map(|t| now.duration_since(*t) >= SESSION_IDLE)
                        .unwrap_or(true)
            })
            .cloned()
            .collect()
    };

    if expired.is_empty() {
        return;
    }
    let mut hosts = lock_hosts();
    hosts.retain(|_, current| !expired.iter().any(|dead| Arc::ptr_eq(dead, current)));
}

/// Start the idle reaper once, on the first host spawn.
fn ensure_reaper() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(REAP_INTERVAL).await;
                reap_idle(Instant::now());
            }
        });
    });
}

/// One round trip against `command` over its session. A session that turns out
/// to be dead is replaced once — which is also what keeps a one-shot host (the
/// minimal documented contract) working.
async fn rpc_call(command: &str, request: &serde_json::Value) -> Option<serde_json::Value> {
    let mut req_str = serde_json::to_string(request).ok()?;
    req_str.push('\n');
    let id = request.get("id").cloned();

    for attempt in 0..2 {
        let host = session_for(command)?;
        let mut session = host.session.lock().await;
        match session.exchange(&req_str, id.as_ref()).await {
            Ok(value) => {
                drop(session);
                if let Ok(mut last) = host.last_used.lock() {
                    *last = Instant::now();
                }
                return Some(value);
            }
            Err(ExchangeError::Timeout) => {
                drop(session);
                // a timed-out exchange leaves an unread response behind
                drop_session(command, &host);
                return None;
            }
            Err(ExchangeError::Dead) => {
                drop(session);
                drop_session(command, &host);
                if attempt == 1 {
                    return None;
                }
            }
        }
    }
    None
}

/// One request against a throwaway process: spawn, write, close stdin, reap.
/// Used for identity discovery, which must not leave a host resident. The wait
/// is bounded — a host that answers and then lingers would otherwise block the
/// first search forever.
async fn one_shot_call(command: &str, request: &serde_json::Value) -> Option<serde_json::Value> {
    let mut req_str = serde_json::to_string(request).ok()?;
    req_str.push('\n');

    let mut child = Command::new(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?; // command not found -> no host

    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(req_str.as_bytes()).await.is_err() {
            let _ = child.kill().await;
            return None;
        }
        drop(stdin); // close stdin so the host sees EOF (one-shot)
    }

    // read to EOF under a deadline: `wait_with_output` would take the child and
    // leave the timeout path unable to kill it
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill().await;
        return None;
    };
    let read = async {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    };
    let Ok(Ok(buf)) = tokio::time::timeout(DISCOVERY_TIMEOUT, read).await else {
        let _ = child.kill().await;
        return None;
    };

    // stdout closed: the answer is complete. Reap, but not forever.
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;

    let out = String::from_utf8_lossy(&buf);
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
}

/// Ask the host who it serves. Returns every plugin it describes via
/// `list_plugins`; empty when the host is missing or does not self-describe.
pub async fn discover(command: &str) -> Vec<HostMeta> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "list_plugins",
        "id": host_request_id(),
    });
    let Some(response) = one_shot_call(command, &request).await else {
        return Vec::new();
    };
    let Some(list) = response.get("result").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            Some(HostMeta {
                id: entry.get("id")?.as_str()?.to_string(),
                name: entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                icon: entry
                    .get("icon")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                ready: entry
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Normalize a host response's `result` array into rows, resolving each
/// icon to what the UI can render. `None` when the response has no usable
/// `result` array.
fn parse_result_items(response: &serde_json::Value, icon: &str) -> Option<Vec<ResultItem>> {
    let items = response.get("result")?.as_array()?;
    let mut parsed: Vec<ResultItem> = items
        .iter()
        .filter_map(|it| serde_json::from_value(it.clone()).ok())
        .collect();
    let icon_path = find_icon_path(icon);
    for item in &mut parsed {
        let spec = item.icon.as_deref().unwrap_or("");
        item.icon = resolve_item_icon(spec, icon_path.clone());
    }
    Some(parsed)
}

async fn query_external(
    command: &str,
    plugin: &str,
    text: &str,
    icon: &str,
) -> Result<Vec<ResultItem>> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "search",
        "params": { "plugin": plugin, "text": text },
        "id": host_request_id(),
    });
    let Some(response) = rpc_call(command, &request).await else {
        return Ok(Vec::new());
    };
    Ok(parse_result_items(&response, icon).unwrap_or_default())
}

/// Ask the host for its default view (its `top` method). `Ok(None)` when the
/// host has no such method (`-32601`), it errored, or produced no usable
/// result — the caller falls back to the identity card.
async fn query_default(command: &str, plugin: &str, icon: &str) -> Result<Option<Vec<ResultItem>>> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "top",
        "params": { "plugin": plugin },
        "id": host_request_id(),
    });
    let Some(response) = rpc_call(command, &request).await else {
        return Ok(None);
    };
    if response.get("error").is_some() {
        return Ok(None);
    }
    Ok(parse_result_items(&response, icon))
}

/// First shell token of a `run:` payload (argv0), or `None` for any other
/// scheme. Hosts emit single-token commands (plugins.toml contract), so a
/// plain whitespace split is sufficient.
fn run_argv0(on_click: &str) -> Option<&str> {
    let rest = on_click.strip_prefix("run:")?;
    if rest.is_empty() {
        return None;
    }
    rest.split_whitespace().next()
}

/// Resolve a plugins.toml `command` to an absolute path when it is a bare
/// name (PATH lookup); absolute paths pass through unchanged.
fn resolve_command(command: &str) -> String {
    if command.contains('/') {
        return command.to_string();
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(command);
            if candidate.is_file() {
                return candidate.display().to_string();
            }
        }
    }
    command.to_string()
}

/// Relay a row's removal to the host that owns it: when `on_click` is a
/// `run:` shell command whose first token is this host's `command` (as
/// configured or resolved on PATH), ask the host to `forget` the row — it
/// may delete plugin data (e.g. a todo item). Host failures are silent:
/// usage history was already removed, and a host without a `forget` method
/// simply has no data to drop.
async fn forget_external(command: &str, on_click: &str) -> Result<()> {
    let Some(argv0) = run_argv0(on_click) else {
        return Ok(());
    };
    // Compare the cheap form first: resolving a bare command walks $PATH.
    if argv0 != command && argv0 != resolve_command(command) {
        return Ok(());
    }
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "forget",
        "params": { "on_click": on_click },
        "id": host_request_id(),
    });
    let _ = rpc_call(command, &request).await;
    Ok(())
}
/// Resolve one result icon to what the UI can render (`file://` + path):
/// empty -> the plugin's own icon, `papirus:` -> absolute Papirus path,
/// anything else (already an absolute path) passes through.
fn resolve_item_icon(icon: &str, fallback: Option<String>) -> Option<String> {
    if icon.is_empty() {
        return fallback;
    }
    if icon.starts_with("papirus:") {
        return find_icon_path(icon);
    }
    Some(icon.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn empty_icon_falls_back_to_plugin_icon() {
        assert_eq!(
            resolve_item_icon("", Some("/x.png".into())),
            Some("/x.png".into())
        );
        assert_eq!(resolve_item_icon("", None), None);
    }

    #[test]
    fn absolute_path_passes_through() {
        assert_eq!(
            resolve_item_icon("/home/u/icon.svg", None),
            Some("/home/u/icon.svg".into())
        );
    }

    #[test]
    fn papirus_spec_is_resolved_to_absolute_path() {
        if !Path::new("/usr/share/icons/Papirus").exists() {
            return; // theme not installed on this machine
        }
        let resolved = resolve_item_icon("papirus:folder-open", None).unwrap();
        assert!(resolved.contains("/Papirus/"));
        assert!(resolved.ends_with(".svg"));
    }

    /// A line-oriented host must serve every request from one process.
    fn mock_host(dir: &Path, spawns: &Path, id: u64) -> String {
        use std::os::unix::fs::PermissionsExt;

        let script = dir.join("host.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf 'x\\n' >> '{spawns}'\n\
                 while IFS= read -r line; do\n\
                 printf '{{\"jsonrpc\":\"2.0\",\"result\":[{{\"title\":\"ok\"}}],\"id\":{id}}}\\n'\n\
                 done\n",
                spawns = spawns.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script.display().to_string()
    }

    fn request(id: u64) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "search",
            "params": { "plugin": "mock", "text": "a" },
            "id": id,
        })
    }

    #[tokio::test]
    async fn one_host_process_serves_many_requests() {
        let dir = tempfile::tempdir().unwrap();
        let spawns = dir.path().join("spawns");
        let command = mock_host(dir.path(), &spawns, 4242);
        let request = request(4242);

        for _ in 0..3 {
            let response = rpc_call(&command, &request).await.expect("host answered");
            assert_eq!(response["result"][0]["title"], "ok");
        }

        let spawns = std::fs::read_to_string(&spawns).unwrap();
        assert_eq!(spawns.lines().count(), 1, "host was respawned: {spawns}");
    }

    /// A one-shot host must keep working: the dead session is replaced once.
    #[tokio::test]
    async fn one_shot_host_is_respawned_per_request() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let spawns = dir.path().join("spawns");
        let script = dir.path().join("once.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf 'x\\n' >> '{spawns}'\n\
                 IFS= read -r line\n\
                 printf '{{\"jsonrpc\":\"2.0\",\"result\":[{{\"title\":\"once\"}}],\"id\":9}}\\n'\n",
                spawns = spawns.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let command = script.display().to_string();
        let request = request(9);

        for _ in 0..2 {
            let response = rpc_call(&command, &request).await.expect("host answered");
            assert_eq!(response["result"][0]["title"], "once");
        }

        let spawns = std::fs::read_to_string(&spawns).unwrap();
        assert_eq!(spawns.lines().count(), 2, "one-shot host was not respawned");
    }

    #[tokio::test]
    async fn stale_responses_are_skipped_by_id() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("stale.sh");
        // answers with the previous id first: the stale one must be skipped
        std::fs::write(
            &script,
            "#!/bin/sh\nwhile IFS= read -r line; do\n\
             printf '{\"jsonrpc\":\"2.0\",\"result\":[{\"title\":\"stale\"}],\"id\":1}\\n'\n\
             printf '{\"jsonrpc\":\"2.0\",\"result\":[{\"title\":\"fresh\"}],\"id\":77}\\n'\ndone\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let response = rpc_call(&script.display().to_string(), &request(77))
            .await
            .expect("host answered");
        assert_eq!(response["result"][0]["title"], "fresh");
    }

    #[tokio::test]
    async fn a_missing_host_is_not_an_error() {
        assert!(rpc_call("/nonexistent/qsflow-host", &request(1)).await.is_none());
        assert!(one_shot_call("/nonexistent/qsflow-host", &request(1))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn idle_sessions_are_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let spawns = dir.path().join("spawns");
        let command = mock_host(dir.path(), &spawns, 5);

        let host = session_for(&command).unwrap();
        assert!(lock_hosts().contains_key(&command));

        // still fresh: not reaped
        reap_idle(Instant::now());
        assert!(lock_hosts().contains_key(&command));

        // pretend the last request was long ago (other tests' fresh sessions
        // stay untouched)
        *host.last_used.lock().unwrap() = Instant::now() - SESSION_IDLE - Duration::from_secs(1);
        drop(host);
        reap_idle(Instant::now());
        assert!(!lock_hosts().contains_key(&command));
    }
}
