//! Installs from the MCP server (D-318): what an install would fetch, and the one registry of
//! install jobs that answers both a client with the tasks extension (`tasks/get`) and any
//! other (`install_status`). A job runs on its own thread through the store's client; one
//! install runs at a time, as the store has one writer.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rmcp::model::{CallToolResult, DetailedTask, Task, TaskPayload, TaskStatus};
use rmcp::task_manager::current_timestamp;
use serde_json::{Value, json};
use teigeodata_engine::Component;
use teigeodata_sync::catalog::Catalog;
use teigeodata_sync::client::{self, Client, ClientError, PackState, Want};

use crate::packs::Packs;
use crate::query::{Code, QueryError};

/// How many jobs the registry keeps, ended ones included.
pub const KEPT: usize = 16;
/// How long a job is kept after it ends, in milliseconds (a task's `ttlMs`).
pub const TTL_MS: u64 = 3_600_000;
/// How often a client should ask about a job, in milliseconds (a task's `pollIntervalMs`).
pub const POLL_MS: u64 = 1_000;
/// The download an install may fetch without the person's confirmation (`max_bytes`).
pub const DEFAULT_MAX_BYTES: u64 = 100_000_000;

/// What an install would do: the catalog paths, the components, and the bytes to download.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub(crate) paths: Vec<String>,
    pub(crate) want: Want,
    pub(crate) download: u64,
}

/// What installing `packs` (catalog paths, or addresses such as `np` or `np.places`) with
/// `components` (every one when empty) into the store at `root` would download: the bytes of
/// each pack not installed with those components already.
///
/// # Errors
/// A pack not in the catalog, a component not known, or the catalog not readable.
pub(crate) fn plan(
    root: &Path,
    packs: &[String],
    components: &[String],
) -> Result<Plan, QueryError> {
    if packs.is_empty() {
        return Err(QueryError::new(Code::InvalidArgument, "`packs` is empty"));
    }
    let want = Want {
        components: components
            .iter()
            .map(|c| {
                Component::from_name(c).ok_or_else(|| {
                    QueryError::new(Code::InvalidArgument, format!("no such component: {c:?}"))
                })
            })
            .collect::<Result<_, _>>()?,
        ..Want::default()
    };
    let set = want.set();
    let (trusted, installed) = client::catalog_at(root).map_err(|_| unreadable())?;
    let (catalog, _) = Catalog::trusted(&trusted);
    let mut paths = Vec::with_capacity(packs.len());
    let mut download = 0_u64;
    for p in packs {
        let path = client::path_of(p);
        let entry = catalog.get(&path).ok_or_else(|| {
            QueryError::new(
                Code::NotFound,
                format!("{p}: not in the catalog (`coverage` lists the packs for a point)"),
            )
        })?;
        let has = installed
            .iter()
            .find(|r| r.address == entry.address && r.component == entry.component);
        let current = PackState::of(entry, &installed) == PackState::Installed
            && has.is_some_and(|r| r.set.contains_all(set));
        if !current {
            download = download.saturating_add(entry.size_of(set).wire);
        }
        paths.push(path);
    }
    Ok(Plan {
        paths,
        want,
        download,
    })
}

fn unreadable() -> QueryError {
    QueryError::new(Code::Unavailable, "the store's catalog cannot be read")
}

/// Megabytes for people, one decimal.
pub(crate) fn megabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss, reason = "a size for people")]
    let mb = bytes as f64 / 1e6;
    format!("{mb:.1} MB")
}

/// One install: its ID, when it began, whether a cancel was asked, and where it is.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) id: String,
    created: String,
    cancel: AtomicBool,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    status: TaskStatus,
    message: String,
    updated: String,
    ended: Option<Instant>,
    /// The finished call's result: a tool result, an error one when the install failed.
    result: Option<CallToolResult>,
    /// The failure, for `install_status`.
    error: Option<QueryError>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A job's state stays consistent under a panic in another holder: each write is whole.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Job {
    fn set(&self, status: TaskStatus, message: impl Into<String>) {
        let mut s = lock(&self.state);
        s.status = status;
        s.message = message.into();
        s.updated = current_timestamp();
        if status.is_terminal() {
            s.ended = Some(Instant::now());
        }
    }

    fn finish(&self, result: CallToolResult, error: Option<QueryError>, message: String) {
        {
            let mut s = lock(&self.state);
            s.result = Some(result);
            s.error = error;
        }
        self.set(TaskStatus::Completed, message);
    }

    /// The job as a task: `completed` carries the call's result (an error result when the
    /// install failed, as a tool error is a completed call).
    pub(crate) fn task(&self) -> DetailedTask {
        let s = lock(&self.state);
        let task = Task::new(&self.id, s.status, &self.created, &s.updated)
            .with_status_message(&s.message)
            .with_ttl_ms(TTL_MS)
            .with_poll_interval_ms(POLL_MS);
        let payload = match s.status {
            TaskStatus::Completed => TaskPayload::Completed {
                result: s
                    .result
                    .as_ref()
                    .and_then(|r| serde_json::to_value(r).ok())
                    .and_then(|v| match v {
                        Value::Object(o) => Some(o),
                        _ => None,
                    })
                    .unwrap_or_default(),
            },
            TaskStatus::Cancelled => TaskPayload::Cancelled,
            _ => TaskPayload::Working,
        };
        DetailedTask::new(task, payload)
    }

    /// The job for `install_status`: its status (`failed` for a failed install), message, and
    /// its result or error once it has ended.
    pub(crate) fn status(&self) -> Value {
        let s = lock(&self.state);
        let status = match (s.status, &s.error) {
            (TaskStatus::Completed, Some(_)) => "failed",
            (TaskStatus::Completed, None) => "completed",
            (TaskStatus::Cancelled, _) => "cancelled",
            _ => "working",
        };
        json!({
            "handle": self.id,
            "status": status,
            "message": s.message,
            "created": self.created,
            "updated": s.updated,
            "result": s.result.as_ref().filter(|_| s.error.is_none())
                .and_then(|r| r.structured_content.as_ref())
                .and_then(|v| v.get("result")),
            "error": s.error.as_ref().map(|e| json!({ "code": e.code.name(), "message": e.message })),
        })
    }
}

/// The install jobs: at most [`KEPT`], each dropped [`TTL_MS`] after it ends.
#[derive(Debug, Default)]
pub(crate) struct Jobs {
    jobs: Mutex<VecDeque<Arc<Job>>>,
    /// Held by the job installing: the store has one writer.
    turn: Arc<Mutex<()>>,
}

impl Jobs {
    /// Starts installing `plan` into the store at `root`, as job `id`; `packs` reload when it
    /// is done.
    ///
    /// # Errors
    /// [`Code::RateLimited`] when [`KEPT`] jobs are kept and none has ended.
    pub(crate) fn start(
        &self,
        id: String,
        root: PathBuf,
        plan: Plan,
        packs: Arc<Packs>,
    ) -> Result<Arc<Job>, QueryError> {
        let now = current_timestamp();
        let job = Arc::new(Job {
            id,
            created: now.clone(),
            cancel: AtomicBool::new(false),
            state: Mutex::new(State {
                status: TaskStatus::Working,
                message: "waiting for another install".into(),
                updated: now,
                ended: None,
                result: None,
                error: None,
            }),
        });
        {
            let mut jobs = lock(&self.jobs);
            let ttl = Duration::from_millis(TTL_MS);
            jobs.retain(|j| lock(&j.state).ended.is_none_or(|t| t.elapsed() < ttl));
            if jobs.len() >= KEPT {
                let ended = jobs.iter().position(|j| lock(&j.state).ended.is_some());
                match ended {
                    Some(i) => {
                        jobs.remove(i);
                    }
                    None => {
                        return Err(QueryError::new(
                            Code::RateLimited,
                            format!("{KEPT} installs are pending: try again when one ends"),
                        ));
                    }
                }
            }
            jobs.push_back(Arc::clone(&job));
        }
        let (turn, run) = (Arc::clone(&self.turn), Arc::clone(&job));
        std::thread::Builder::new()
            .name("teigeodata-install".into())
            .spawn(move || install(&run, &turn, &root, &plan, &packs))
            .map_err(|_| QueryError::new(Code::Unavailable, "the install could not start"))?;
        Ok(job)
    }

    /// The job `id`, if kept.
    pub(crate) fn get(&self, id: &str) -> Option<Arc<Job>> {
        lock(&self.jobs).iter().find(|j| j.id == id).cloned()
    }

    /// Asks job `id` to stop: one waiting for its turn ends `cancelled`; one installing ends
    /// when the pack in progress is in, so the store stays whole. `false` when not kept.
    pub(crate) fn cancel(&self, id: &str) -> bool {
        self.get(id).is_some_and(|j| {
            j.cancel.store(true, Ordering::Release);
            true
        })
    }
}

/// Runs one install job to its end.
fn install(job: &Job, turn: &Mutex<()>, root: &Path, plan: &Plan, packs: &Packs) {
    let _turn = lock(turn);
    if job.cancel.load(Ordering::Acquire) {
        job.set(TaskStatus::Cancelled, "cancelled before it began");
        return;
    }
    job.set(
        TaskStatus::Working,
        format!(
            "installing {} ({})",
            plan.paths.join(", "),
            megabytes(plan.download)
        ),
    );
    let done =
        Client::open(root).and_then(|(mut c, _)| c.install(&plan.paths, &plan.want, false, None));
    match done {
        Ok(report) => {
            // Serve what was installed now rather than at the next follow.
            let _ = packs.follow();
            let installed: Vec<Value> = report
                .done
                .iter()
                .map(|d| {
                    json!({
                        "path": d.path,
                        "release": d.release,
                        "fetched": d.installed.as_ref().map_or(0, |i| i.fetched),
                        "already": d.installed.is_none(),
                    })
                })
                .collect();
            let message = format!("installed {}", plan.paths.join(", "));
            let result = crate::mcp::done(
                json!({ "result": { "installed": installed }, "attribution": null }),
                None,
            );
            job.finish(result, None, message);
        }
        Err(e) => {
            let error = failure(&e, root);
            let message = error.message.clone();
            job.finish(crate::mcp::failed(&error), Some(error), message);
        }
    }
}

/// A store failure as a query error, its message free of the store's path.
fn failure(e: &ClientError, root: &Path) -> QueryError {
    let code = match e {
        ClientError::Network(_) | ClientError::Cancelled => Code::Unavailable,
        ClientError::Refused(_) => Code::InvalidArgument,
        _ => Code::Internal,
    };
    let message = e.to_string().replace(&*root.to_string_lossy(), "the store");
    QueryError::new(code, message)
}
