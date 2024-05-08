use crate::tasks::ProgressState;
use dashmap::DashMap;
use std::sync::Arc;

pub type ProgressReceiver = tokio::sync::watch::Receiver<ProgressState>;

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

pub struct ProgressSenderAsync {
    name: String,
    tx: tokio::sync::watch::Sender<ProgressState>,
    sub_tasks: DashMap<String, SubTaskProgress>,
}

fn compute_progress(sub_tasks: &DashMap<String, SubTaskProgress>) -> ProgressState {
    let mut progress_msg = None;
    let progress_value = sub_tasks
        .iter()
        .map(|sub_task| match &sub_task.state {
            ProgressState::NotStarted => 0.0,
            ProgressState::Indeterminate => 0.0,
            ProgressState::Determinate(x) => sub_task.weight * x,
            ProgressState::DeterminateWithMessage(x, msg) => {
                progress_msg = Some(msg.clone());
                sub_task.weight * x
            }
            ProgressState::Completed => sub_task.weight,
        })
        .sum();

    if let Some(msg) = progress_msg {
        ProgressState::DeterminateWithMessage(progress_value, msg)
    } else {
        ProgressState::Determinate(progress_value)
    }
}

impl ProgressSenderAsync {
    pub fn new(name: String, tx: tokio::sync::watch::Sender<ProgressState>) -> Box<Arc<Self>> {
        Arc::new(Self {
            name,
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

pub struct DummyProgressSender;

impl DummyProgressSender {
    pub fn new() -> Box<Arc<Self>> {
        Box::new(Arc::new(Self))
    }
}

impl ProgressSender for Arc<DummyProgressSender> {
    fn send(&self, _state: ProgressState) {}
    fn sub_task(&self, _name: &str, _weight: f32) -> ProgressSenderRef {
        Box::new(self.clone())
    }
    fn receive_sub_task_progress(&self, _name: &str, _state: ProgressState) {}
}
