use poll_promise::Promise;

use crate::data::Vault;

mod import;
pub(crate) mod vault;

#[derive(Debug)]
pub enum TaskResult {
    None,
    VaultLoaded(Box<Vault>),
}

#[derive(Debug)]
pub enum TaskError {
    UserCancelled,
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
pub type ProgressSender = tokio::sync::watch::Sender<ProgressState>;

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
        name: String,
        factory: impl FnOnce(ProgressSender) -> Promise<TaskReturn>,
    ) -> Task {
        let (tx, rx) = tokio::sync::watch::channel(ProgressState::NotStarted);
        Task {
            name,
            promise: factory(tx),
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
        name: String,
        factory: impl FnOnce(ProgressSender) -> Promise<TaskReturn>,
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
