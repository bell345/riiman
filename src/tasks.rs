use std::collections::HashSet;
use std::path::Path;

use eframe::egui;
use eframe::egui::ColorImage;
use itertools::Itertools;
use poll_promise::Promise;

use progress::ProgressReceiver;
use progress::ProgressSenderAsync;
pub use progress::ProgressSenderRef;

use crate::data::{DebugViewportClass, ThumbnailParams};
use crate::state::AppStateRef;
pub use crate::tasks::thumb_grid::RiverParams;
pub use crate::tasks::thumb_grid::ThumbnailGridInfo;
use crate::tasks::transform::PathTransformResult;
use crate::ui::QueryResult;

pub(crate) mod choose;
pub(crate) mod download;
pub(crate) mod filter;
mod image;
pub(crate) mod import;
pub(crate) mod link;
mod progress;
pub(crate) mod sort;
pub(crate) mod thumb_grid;
pub(crate) mod thumbnail;
pub(crate) mod transform;
pub(crate) mod vault;

#[derive(Debug)]
pub enum AsyncTaskResult {
    None,
    VaultLoaded {
        name: String,
        set_as_current: bool,
    },
    VaultSaved(String),
    ImportComplete {
        path: Box<Path>,
        results: Vec<SingleImportResult>,
    },
    LinkComplete {
        other_vault_name: String,
        results: Vec<SingleImportResult>,
    },
    ThumbnailLoaded {
        params: ThumbnailParams,
        image: ColorImage,
    },
    FoundGalleryDl {
        path: String,
        version: String,
    },
    PreviewReady {
        id: egui::Id,
        image: ColorImage,
        viewport_class: DebugViewportClass,
    },
    SelectedDirectory(String),
    SelectedFile(String),
    QueryResult(QueryResult),
    PathTransformationComplete(Vec<anyhow::Result<PathTransformResult>>),
}

pub type SingleImportResult = anyhow::Result<Box<Path>>;

#[derive(Debug, Clone, Default)]
pub enum ProgressState {
    #[default]
    NotStarted,
    Determinate(f32),
    DeterminateWithMessage(f32, String),
    Indeterminate,
    Completed,
}

pub type AsyncTaskReturn = anyhow::Result<AsyncTaskResult>;
pub type TaskFactory = Box<
    dyn FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn> + Send + Sync + 'static,
>;

struct Task {
    id: Option<egui::Id>,
    name: String,
    promise: Promise<AsyncTaskReturn>,
    progress_rx: Option<ProgressReceiver>,
}

impl Task {
    pub fn with_progress(
        id: Option<egui::Id>,
        name: String,
        factory: impl FnOnce(ProgressSenderRef) -> Promise<AsyncTaskReturn>,
    ) -> Task {
        let (tx, rx) = tokio::sync::watch::channel(ProgressState::NotStarted);
        Task {
            id,
            promise: factory(ProgressSenderAsync::new(name.clone(), tx)),
            name,
            progress_rx: Some(rx),
        }
    }

    pub fn from_ready(id: egui::Id, value: AsyncTaskReturn) -> Self {
        Self {
            id: Some(id),
            promise: Promise::from_ready(value),
            name: String::new(),
            progress_rx: None,
        }
    }

    pub fn try_take_result(self) -> Result<AsyncTaskReturn, Task> {
        match self.promise.try_take() {
            Ok(result) => Ok(result),
            Err(promise) => Err(Self { promise, ..self }),
        }
    }
}

#[derive(Default)]
pub(crate) struct TaskState {
    running_tasks: Vec<Task>,
    requests: HashSet<egui::Id>,
}

impl TaskState {
    pub fn add(
        &mut self,
        name: String,
        factory: impl FnOnce(ProgressSenderRef) -> Promise<AsyncTaskReturn>,
    ) {
        self.running_tasks
            .push(Task::with_progress(None, name, factory));
    }

    fn clear_tasks_by_id(&mut self, id: egui::Id) {
        let idxs = self
            .running_tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.id.as_ref() == Some(&id))
            .map(|(i, _)| i)
            .rev()
            .collect_vec();

        for idx in idxs {
            self.running_tasks.swap_remove(idx);
        }
    }

    pub fn add_request(
        &mut self,
        id: egui::Id,
        name: String,
        factory: impl FnOnce(ProgressSenderRef) -> Promise<AsyncTaskReturn>,
    ) {
        self.clear_tasks_by_id(id);
        self.requests.insert(id);
        self.running_tasks
            .push(Task::with_progress(Some(id), name, factory));
    }

    pub fn push_completed_task(&mut self, id: egui::Id, value: AsyncTaskReturn) {
        self.clear_tasks_by_id(id);
        self.requests.insert(id);
        self.running_tasks.push(Task::from_ready(id, value));
    }

    pub fn running_tasks_count(&self) -> usize {
        self.running_tasks.len()
    }

    pub fn iter_ready(&mut self) -> (Vec<AsyncTaskReturn>, Vec<(egui::Id, AsyncTaskReturn)>) {
        let mut results = vec![];
        let mut request_results = vec![];
        let mut still_running_tasks = vec![];
        for task in self.running_tasks.drain(..) {
            match (task.id, task.try_take_result()) {
                (Some(id), Ok(result)) => {
                    if self.requests.remove(&id) {
                        request_results.push((id, result));
                    }
                }
                (None, Ok(result)) => results.push(result),
                (_, Err(task)) => still_running_tasks.push(task),
            }
        }

        self.running_tasks = still_running_tasks;
        (results, request_results)
    }

    pub fn iter_progress(&self) -> Vec<(String, ProgressState)> {
        let mut progresses = vec![];
        for task in &self.running_tasks {
            if task.progress_rx.is_some() {
                let rx = task.progress_rx.as_ref().unwrap();
                progresses.push((task.name.clone(), rx.borrow().clone()));
            }
        }
        progresses
    }
}
