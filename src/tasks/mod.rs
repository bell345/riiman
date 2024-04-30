use poll_promise::Promise;
use std::path::Path;

use crate::data::Vault;
pub use crate::tasks::compute::ThumbnailGridInfo;
pub use crate::tasks::compute::ThumbnailGridParams;

pub(crate) mod compute;
pub(crate) mod import;
mod progress;
pub(crate) mod vault;

pub use progress::DummyProgressSender;
use progress::ProgressReceiver;
use progress::ProgressSenderAsync;
pub use progress::ProgressSenderRef;

#[derive(Debug)]
pub enum TaskResult {
    None,
    VaultLoaded(Box<Vault>),
    ImportComplete {
        path: Box<Path>,
        results: Vec<SingleImportResult>,
    },
    ThumbnailGrid(ThumbnailGridInfo),
}

pub type SingleImportResult = anyhow::Result<Box<Path>>;

#[derive(Debug)]
pub enum TaskError {
    UserCancelled,
    WasmNotImplemented,
    Error(anyhow::Error),
}

#[derive(Debug, Clone, Default)]
pub enum ProgressState {
    #[default]
    NotStarted,
    Determinate(f32),
    DeterminateWithMessage(f32, String),
    Indeterminate,
    Completed,
}

impl<T: Into<anyhow::Error>> From<T> for TaskError {
    fn from(value: T) -> Self {
        Self::Error(value.into())
    }
}

pub type TaskReturn = Result<TaskResult, TaskError>;

struct Task {
    name: String,
    promise: Promise<TaskReturn>,
    progress_rx: Option<ProgressReceiver>,
}

impl Task {
    pub fn with_progress(
        name: &str,
        factory: impl FnOnce(ProgressSenderRef) -> Promise<TaskReturn>,
    ) -> Task {
        let (tx, rx) = tokio::sync::watch::channel(ProgressState::NotStarted);
        Task {
            name: name.to_string(),
            promise: factory(ProgressSenderAsync::new(name, tx)),
            progress_rx: Some(rx),
        }
    }

    pub fn try_take_result(self) -> Result<TaskReturn, Task> {
        match self.promise.try_take() {
            Ok(result) => Ok(result),
            Err(promise) => Err(Self { promise, ..self }),
        }
    }
}

#[derive(Default)]
pub(crate) struct TaskState {
    running_tasks: Vec<Task>,
}

impl TaskState {
    pub fn add_task_with_progress(
        &mut self,
        name: &str,
        factory: impl FnOnce(ProgressSenderRef) -> Promise<TaskReturn>,
    ) {
        self.running_tasks.push(Task::with_progress(name, factory));
    }

    pub fn iter_ready(&mut self) -> Vec<TaskReturn> {
        let mut results = vec![];
        let mut still_running_tasks = vec![];
        for task in self.running_tasks.drain(..) {
            match task.try_take_result() {
                Ok(result) => results.push(result),
                Err(task) => still_running_tasks.push(task),
            }
        }

        self.running_tasks = still_running_tasks;
        results
    }

    pub fn iter_progress(&self) -> Vec<(String, ProgressState)> {
        let mut progresses = vec![];
        for task in self.running_tasks.iter() {
            if task.progress_rx.is_some() {
                let rx = task.progress_rx.as_ref().unwrap();
                progresses.push((task.name.clone(), rx.borrow().clone()));
            }
        }
        progresses
    }
}
