//! Worker management — `WorkerInstance`, `WorkerStatus`, and `WorkerRegistry`.
//!
//! A Worker is a running Agent CLI instance. The orchestrator creates workers
//! when it dispatches tasks via `DispatchDirective`. Workers are assigned a
//! unique `worker_name` (e.g. `oc_login-fix(frontend)`) that the LLM generates
//! based on task context.
//!
//! The `WorkerRegistry` tracks all active and recent workers, supports sending
//! text directly to a worker's herdr pane (for `@` mention routing), and
//! provides the data source for the `@` popup and supervision UI.

use std::sync::{Arc, Mutex};

/// Maximum completed workers retained in memory.
#[allow(dead_code)]
const MAX_COMPLETED_WORKERS: usize = 20;

/// A running agent worker instance.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorkerInstance {
    /// Unique numeric ID assigned by the registry.
    pub id: usize,
    /// Human-readable worker name (e.g. "oc_login-fix(frontend)").
    pub worker_name: String,
    /// Agent CLI name (e.g. "opencode", "claude").
    pub cli_name: String,
    /// Role ID that this worker was assigned (e.g. "frontend-developer").
    pub role_id: String,
    /// herdr pane ID where this worker runs.
    pub pane_id: String,
    /// herdr terminal ID.
    pub terminal_id: String,
    /// Current status.
    pub status: WorkerStatus,
    /// Accumulated output from the worker pane.
    pub output_history: String,
    /// Timestamp when this worker was created.
    pub created_at: String,
}

/// Worker execution status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WorkerStatus {
    /// Agent CLI is starting up.
    Starting,
    /// Agent is executing a task.
    Running,
    /// Agent completed successfully.
    Done,
    /// Agent encountered an error.
    Error(String),
    /// Agent was cancelled by the user.
    Cancelled,
}

impl std::fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerStatus::Starting => write!(f, "starting"),
            WorkerStatus::Running => write!(f, "running"),
            WorkerStatus::Done => write!(f, "done"),
            WorkerStatus::Error(msg) => write!(f, "error: {msg}"),
            WorkerStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Thread-safe registry for tracking worker instances.
#[derive(Debug)]
pub struct WorkerRegistry {
    inner: Arc<Mutex<WorkerRegistryInner>>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct WorkerRegistryInner {
    workers: Vec<WorkerInstance>,
    next_id: usize,
}

impl Default for WorkerRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(WorkerRegistryInner {
                workers: Vec::new(),
                next_id: 1,
            })),
        }
    }
}

impl Clone for WorkerRegistry {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[allow(dead_code)]
impl WorkerRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new worker and return its assigned ID.
    pub fn register(
        &self,
        worker_name: String,
        cli_name: String,
        role_id: String,
        pane_id: String,
        terminal_id: String,
    ) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let id = inner.next_id;
        inner.next_id += 1;
        let now = crate::util::timestamp_now();
        inner.workers.push(WorkerInstance {
            id,
            worker_name,
            cli_name,
            role_id,
            pane_id,
            terminal_id,
            status: WorkerStatus::Starting,
            output_history: String::new(),
            created_at: now,
        });
        id
    }

    /// Update a worker's status by its numeric ID.
    pub fn update_status(&self, id: usize, status: WorkerStatus) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(worker) = inner.workers.iter_mut().find(|w| w.id == id) {
            worker.status = status;
        }
    }

    /// Append output text to a worker's history by pane ID.
    pub fn append_output(&self, pane_id: &str, text: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(worker) = inner.workers.iter_mut().find(|w| w.pane_id == pane_id) {
            worker.output_history.push_str(text);
        }
    }

    /// Set a worker's output history by pane ID.
    pub fn set_output(&self, pane_id: &str, text: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(worker) = inner.workers.iter_mut().find(|w| w.pane_id == pane_id) {
            worker.output_history = text.to_string();
        }
    }

    /// Get a worker by its numeric ID.
    pub fn get(&self, id: usize) -> Option<WorkerInstance> {
        let inner = self.inner.lock().unwrap();
        inner.workers.iter().find(|w| w.id == id).cloned()
    }

    /// Find a worker by pane ID.
    pub fn get_by_pane(&self, pane_id: &str) -> Option<WorkerInstance> {
        let inner = self.inner.lock().unwrap();
        inner.workers.iter().find(|w| w.pane_id == pane_id).cloned()
    }

    /// Get all currently active (starting or running) workers.
    pub fn active_workers(&self) -> Vec<WorkerInstance> {
        let inner = self.inner.lock().unwrap();
        inner
            .workers
            .iter()
            .filter(|w| matches!(w.status, WorkerStatus::Starting | WorkerStatus::Running))
            .cloned()
            .collect()
    }

    /// Get all workers (active + completed).
    pub fn all_workers(&self) -> Vec<WorkerInstance> {
        let inner = self.inner.lock().unwrap();
        inner.workers.clone()
    }

    /// Mark a worker as cancelled by its pane ID.
    pub fn cancel_by_pane(&self, pane_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(worker) = inner.workers.iter_mut().find(|w| w.pane_id == pane_id)
            && matches!(
                worker.status,
                WorkerStatus::Starting | WorkerStatus::Running
            )
        {
            worker.status = WorkerStatus::Cancelled;
        }
    }

    /// Remove old completed workers when the list exceeds the cap.
    pub fn evict_old_completed(&self) {
        let mut inner = self.inner.lock().unwrap();
        let mut active: Vec<WorkerInstance> = Vec::new();
        let mut completed: Vec<WorkerInstance> = Vec::new();
        for w in inner.workers.drain(..) {
            if matches!(w.status, WorkerStatus::Starting | WorkerStatus::Running) {
                active.push(w);
            } else {
                completed.push(w);
            }
        }
        completed.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        completed.truncate(MAX_COMPLETED_WORKERS);
        active.extend(completed);
        inner.workers = active;
    }

    /// Count active workers (starting or running).
    pub fn active_count(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner
            .workers
            .iter()
            .filter(|w| matches!(w.status, WorkerStatus::Starting | WorkerStatus::Running))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> WorkerRegistry {
        WorkerRegistry::new()
    }

    #[test]
    fn test_register_worker() {
        let reg = make_registry();
        let id = reg.register(
            "oc_test(frontend)".into(),
            "opencode".into(),
            "frontend-developer".into(),
            "pane-1".into(),
            "term-1".into(),
        );
        assert_eq!(id, 1);
        let w = reg.get(id).unwrap();
        assert_eq!(w.worker_name, "oc_test(frontend)");
        assert_eq!(w.status.to_string(), "starting");
    }

    #[test]
    fn test_update_status() {
        let reg = make_registry();
        let id = reg.register(
            "w1".into(),
            "c".into(),
            "r".into(),
            "p1".into(),
            "t1".into(),
        );
        reg.update_status(id, WorkerStatus::Running);
        assert_eq!(reg.get(id).unwrap().status, WorkerStatus::Running);
    }

    #[test]
    fn test_append_output() {
        let reg = make_registry();
        reg.register(
            "w1".into(),
            "c".into(),
            "r".into(),
            "p1".into(),
            "t1".into(),
        );
        reg.append_output("p1", "hello");
        reg.append_output("p1", " world");
        assert_eq!(reg.get_by_pane("p1").unwrap().output_history, "hello world");
    }

    #[test]
    fn test_active_workers() {
        let reg = make_registry();
        reg.register("a".into(), "c".into(), "r".into(), "p1".into(), "t1".into());
        let id2 = reg.register("b".into(), "c".into(), "r".into(), "p2".into(), "t2".into());
        reg.update_status(id2, WorkerStatus::Done);
        assert_eq!(reg.active_workers().len(), 1);
        assert_eq!(reg.active_workers()[0].worker_name, "a");
    }

    #[test]
    fn test_cancel_by_pane() {
        let reg = make_registry();
        reg.register("a".into(), "c".into(), "r".into(), "p1".into(), "t1".into());
        reg.cancel_by_pane("p1");
        assert_eq!(
            reg.get_by_pane("p1").unwrap().status,
            WorkerStatus::Cancelled
        );
    }

    #[test]
    fn test_active_count() {
        let reg = make_registry();
        reg.register("a".into(), "c".into(), "r".into(), "p1".into(), "t1".into());
        let id2 = reg.register("b".into(), "c".into(), "r".into(), "p2".into(), "t2".into());
        reg.update_status(id2, WorkerStatus::Done);
        assert_eq!(reg.active_count(), 1);
    }

    #[test]
    fn test_all_workers_includes_completed() {
        let reg = make_registry();
        reg.register("a".into(), "c".into(), "r".into(), "p1".into(), "t1".into());
        assert_eq!(reg.all_workers().len(), 1);
    }
}
