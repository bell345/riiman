use poll_promise::Promise;

use crate::data::Vault;

pub(crate) mod io;

pub enum TaskResult {
    None,
    VaultLoaded(Box<Vault>),
}

pub enum TaskError {
    UserCancelled,
    Error(anyhow::Error),
}

impl<T: Into<anyhow::Error>> From<T> for TaskError {
    fn from(value: T) -> Self {
        Self::Error(value.into())
    }
}

pub type TaskReturn = Result<TaskResult, TaskError>;

pub type Task = Promise<TaskReturn>;

#[derive(Default)]
pub(crate) struct TaskState {
    running_tasks: Vec<Task>,
}

impl TaskState {
    pub fn add_task(&mut self, func: Task) {
        self.running_tasks.push(func);
    }

    pub fn iter_ready(&mut self) -> Vec<TaskReturn> {
        let mut results = vec![];
        let mut still_running_tasks = vec![];
        for task in self.running_tasks.drain(..) {
            match task.try_take() {
                Ok(result) => results.push(result),
                Err(task) => still_running_tasks.push(task),
            }
        }

        self.running_tasks = still_running_tasks;
        results
    }
}
