use dashmap::DashMap;
use std::path::Path;
use std::sync::Arc;

use poll_promise::Promise;

use crate::data::Vault;

pub(crate) mod import;
pub(crate) mod vault;

#[derive(Debug)]
pub enum TaskResult {
    None,
    VaultLoaded(Box<Vault>),
    ImportComplete {
        path: Box<Path>,
        results: Vec<SingleImportResult>,
    },
}

pub type SingleImportResult = anyhow::Result<Box<Path>>;

#[derive(Debug)]
pub enum TaskError {
    UserCancelled,
    WasmNotImplemented,
    Error(anyhow::Error),
}

#[derive(Debug, Clone, Copy, Default)]
pub enum ProgressState {
    #[default]
    NotStarted,
    Determinate(f32),
    Indeterminate,
    Completed,
}

type ProgressReceiver = tokio::sync::watch::Receiver<ProgressState>;
//pub type ProgressSender = tokio::sync::watch::Sender<ProgressState>;

pub trait ProgressSender: Send + Sync {
    fn send(&self, state: ProgressState);
    fn sub_task(&self, name: &str, weight: f32) -> ProgressSenderRef;
    fn receive_sub_task_progress(&self, name: &str, state: ProgressState);
}

pub type ProgressSenderRef = Box<dyn ProgressSender>;

struct SubTaskProgress {
    weight: f32,
    state: ProgressState,
}

struct ProgressSenderAsync {
    name: String,
    tx: tokio::sync::watch::Sender<ProgressState>,
    sub_tasks: DashMap<String, SubTaskProgress>,
}

fn compute_progress(sub_tasks: &DashMap<String, SubTaskProgress>) -> ProgressState {
    ProgressState::Determinate(
        sub_tasks
            .iter()
            .map(|sub_task| match sub_task.state {
                ProgressState::NotStarted => 0.0,
                ProgressState::Indeterminate => 0.0,
                ProgressState::Determinate(x) => sub_task.weight * x,
                ProgressState::Completed => sub_task.weight,
            })
            .sum(),
    )
}

impl ProgressSenderAsync {
    fn new(name: &str, tx: tokio::sync::watch::Sender<ProgressState>) -> Box<Arc<Self>> {
        Arc::new(Self {
            name: name.to_string(),
            tx,
            sub_tasks: DashMap::new(),
        })
        .into()
    }
}

impl ProgressSender for Arc<ProgressSenderAsync> {
    fn send(&self, state: ProgressState) {
        self.tx.send_replace(state);
    }
    fn sub_task(&self, name: &str, weight: f32) -> ProgressSenderRef {
        self.sub_tasks.insert(
            name.to_string(),
            SubTaskProgress {
                weight,
                state: ProgressState::NotStarted,
            },
        );
        Box::new(Arc::new(ProgressSenderSubTask::new(
            Box::new(self.clone()),
            name,
        )))
    }
    fn receive_sub_task_progress(&self, name: &str, state: ProgressState) {
        self.sub_tasks.get_mut(name).unwrap().state = state;
        self.send(compute_progress(&self.sub_tasks));
    }
}

struct ProgressSenderSubTask {
    parent: Box<dyn ProgressSender>,
    name: String,
    sub_tasks: DashMap<String, SubTaskProgress>,
}

impl ProgressSenderSubTask {
    pub fn new(parent: Box<dyn ProgressSender>, name: &str) -> Self {
        ProgressSenderSubTask {
            parent,
            name: name.to_string(),
            sub_tasks: DashMap::new(),
        }
    }
}

impl ProgressSender for Arc<ProgressSenderSubTask> {
    fn send(&self, state: ProgressState) {
        self.parent
            .receive_sub_task_progress(self.name.as_str(), state);
    }
    fn sub_task(&self, name: &str, weight: f32) -> ProgressSenderRef {
        self.sub_tasks.insert(
            name.to_string(),
            SubTaskProgress {
                weight,
                state: ProgressState::NotStarted,
            },
        );
        Box::new(Arc::new(ProgressSenderSubTask::new(
            Box::new(self.clone()),
            name,
        )))
    }
    fn receive_sub_task_progress(&self, name: &str, state: ProgressState) {
        self.sub_tasks.get_mut(name).unwrap().state = state;
        self.send(compute_progress(&self.sub_tasks));
    }
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
                progresses.push((task.name.clone(), *rx.borrow()));
            }
        }
        progresses
    }
}
