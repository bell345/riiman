use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::data::{
    kind, FieldStore, FilterExpression, Item, ItemCache, ItemId, KnownField, ShortcutBehaviour,
    ThumbnailCache, ThumbnailCacheItem, ThumbnailParams, Vault,
};
use crate::errors::AppError;
use crate::fields;
use crate::tasks::sort::{SortDirection, SortExpression};
use crate::tasks::{AsyncTaskReturn, ProgressSenderRef, TaskFactory};
use crate::ui::AppModal;
use chrono::TimeDelta;
use dashmap::{DashMap, DashSet};
use eframe::egui;
use eframe::egui::KeyboardShortcut;
use indexmap::IndexMap;
use poll_promise::Promise;

const THUMBNAIL_CACHE_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const THUMBNAIL_LOAD_INTERVAL_MS: i64 = 50;
const THUMBNAIL_LQ_LOAD_INTERVAL_MS: i64 = 10;
pub const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;

pub enum TaskInfo {
    GlobalTask {
        name: String,
        task_factory: TaskFactory,
    },
    GlobalMessage {
        result: AsyncTaskReturn,
    },
    TaskRequest {
        id: egui::Id,
        name: String,
        task_factory: TaskFactory,
    },
    CompletedTaskRequest {
        id: egui::Id,
        result: AsyncTaskReturn,
    },
}

pub(crate) struct AppState {
    task_queue: Mutex<Vec<TaskInfo>>,
    results: DashMap<egui::Id, AsyncTaskReturn>,
    error_queue: Mutex<Vec<anyhow::Error>>,
    dialog_queue: Mutex<Vec<Box<dyn AppModal>>>,
    vaults: DashMap<String, Arc<Vault>>,
    unresolved_vaults: DashSet<String>,
    current_vault_name: Mutex<Option<String>>,
    vault_loading: AtomicBool,

    shortcuts: Mutex<IndexMap<KeyboardShortcut, ShortcutBehaviour>>,

    thumbnail_cache: ThumbnailCache,
    thumbnail_cache_lq: ThumbnailCache,

    filter: Mutex<FilterExpression>,
    sorts: Mutex<Vec<SortExpression>>,

    filtered_item_list: ItemCache,
    item_list_is_new: AtomicBool,

    selected_item_ids: Mutex<Vec<ItemId>>,
}

impl Debug for AppState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

macro_rules! shortcut {
    ($modifier:ident + $key:ident) => {
        KeyboardShortcut::new(egui::Modifiers::$modifier, egui::Key::$key)
    };
    ($key:ident) => {
        KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::$key)
    };
}

const DEFAULT_SHORTCUTS: [KeyboardShortcut; 10] = [
    shortcut!(CTRL + Num1),
    shortcut!(CTRL + Num2),
    shortcut!(CTRL + Num3),
    shortcut!(CTRL + Num4),
    shortcut!(CTRL + Num5),
    shortcut!(CTRL + Num6),
    shortcut!(CTRL + Num7),
    shortcut!(CTRL + Num8),
    shortcut!(CTRL + Num9),
    shortcut!(CTRL + Num0),
];

impl Default for AppState {
    fn default() -> Self {
        let res = Self {
            task_queue: Default::default(),
            results: Default::default(),
            error_queue: Default::default(),
            dialog_queue: Default::default(),
            vaults: Default::default(),
            unresolved_vaults: Default::default(),
            current_vault_name: Default::default(),
            vault_loading: Default::default(),
            shortcuts: Default::default(),
            thumbnail_cache: ThumbnailCache::new(
                THUMBNAIL_CACHE_SIZE,
                TimeDelta::milliseconds(THUMBNAIL_LOAD_INTERVAL_MS),
                false,
            ),
            thumbnail_cache_lq: ThumbnailCache::new(
                THUMBNAIL_CACHE_SIZE,
                TimeDelta::milliseconds(THUMBNAIL_LQ_LOAD_INTERVAL_MS),
                true,
            ),
            filter: Mutex::new(FilterExpression::TagMatch(fields::image::NAMESPACE.id)),
            sorts: Mutex::new(vec![SortExpression::Path(SortDirection::Ascending)]),
            filtered_item_list: Default::default(),
            item_list_is_new: Default::default(),
            selected_item_ids: Default::default(),
        };

        {
            let mut shortcuts = res.shortcuts.lock().unwrap();
            for shortcut in DEFAULT_SHORTCUTS {
                shortcuts.insert(shortcut, Default::default());
            }
        }

        res
    }
}

impl AppState {
    #[tracing::instrument]
    pub fn load_vault(&self, vault: Vault, set_as_current: bool) {
        let name = vault.name.clone().into();

        for item in vault.iter_items() {
            if let Ok(Some((ref_name, _))) = item.get_known_field_value(fields::general::LINK) {
                if !self.vaults.contains_key(&*ref_name) && ref_name != name {
                    self.unresolved_vaults.insert(ref_name.to_string());
                }
            }
        }

        self.unresolved_vaults.remove(&*name);
        self.vaults.insert(name.to_string(), Arc::new(vault));
        if set_as_current {
            self.set_current_vault_name(name.into())
                .expect("vault we just added should exist");
        }
    }

    pub fn refresh_unresolved_vaults(&self) {
        self.unresolved_vaults.clear();
        for vault in &self.vaults {
            for item in vault.iter_items() {
                if let Ok(links) = item.links() {
                    for kind::ItemRef((ref_name, _)) in links {
                        if !self.vaults.contains_key(&*ref_name) {
                            self.unresolved_vaults.insert(ref_name.to_string());
                        }
                    }
                }
            }
        }
    }

    pub fn get_vault(&self, name: &str) -> Result<Arc<Vault>, AppError> {
        self.vaults
            .get(name)
            .map(|r| r.clone())
            .ok_or(AppError::VaultDoesNotExist {
                name: name.to_owned(),
            })
    }

    #[tracing::instrument]
    pub fn unload_vault(&self, name: &str) -> Result<(), AppError> {
        if self.current_vault_name().as_deref() == Some(name) {
            return Err(AppError::CannotRemoveCurrentVault {
                current_vault_name: name.to_owned(),
            });
        }
        self.vaults.remove(name);
        self.refresh_unresolved_vaults();

        Ok(())
    }

    pub fn current_vault_name(&self) -> Option<String> {
        self.current_vault_name.lock().unwrap().clone()
    }

    pub fn current_vault_opt(&self) -> Option<Arc<Vault>> {
        let name = self.current_vault_name()?;
        let vault = self.vaults.get(&name)?;
        Some(vault.clone())
    }

    pub fn current_vault(&self) -> Result<Arc<Vault>, AppError> {
        self.current_vault_opt().ok_or(AppError::NoCurrentVault)
    }

    pub fn set_current_vault_name(&self, name: String) -> Result<(), AppError> {
        if !self.vaults.contains_key(&name) {
            return Err(AppError::VaultDoesNotExist { name });
        }

        let mut l = self.current_vault_name.lock().unwrap();
        *l = Some(name);

        Ok(())
    }

    pub fn has_unresolved_vaults(&self) -> bool {
        !self.unresolved_vaults.is_empty()
    }

    pub fn valid_vault_names(&self) -> Vec<String> {
        self.vaults.iter().map(|v| v.name.clone()).collect()
    }

    pub fn known_vault_names(&self) -> Vec<String> {
        self.vaults
            .iter()
            .map(|v| v.name.clone())
            .chain(self.unresolved_vaults.iter().map(|n| n.clone()))
            .collect()
    }

    pub fn vault_name_to_file_paths(&self) -> HashMap<String, String> {
        self.vaults
            .iter()
            .filter_map(|r| Some((r.name.clone(), r.file_path.clone()?.to_str()?.to_string())))
            .collect()
    }

    pub fn resolve_link(&self, link: kind::ItemRef) -> Option<(Arc<Vault>, Arc<Item>)> {
        let kind::ItemRef((other_vault_name, other_path)) = link;
        let vault = self.get_vault(&other_vault_name).ok()?;
        let item = vault.get_item(Path::new(&other_path)).ok()?;
        Some((vault, item))
    }

    pub fn update_item_link(
        &self,
        vault: &Vault,
        item: &Item,
        other_vault_name: &str,
        other_path: &str,
    ) -> anyhow::Result<()> {
        let other_vault = self.get_vault(other_vault_name)?;
        let other_item = other_vault.get_item(Path::new(other_path))?;

        for (def, value) in item.cloned_fields_with_defs(vault) {
            let id = def.id;
            if def.has_field(&fields::meta::NO_LINK.id) {
                continue;
            }

            other_vault.set_definition(def);
            other_item.set_field_value(id, value);
        }

        let mut fields_to_remove = vec![];
        for field in other_item.iter_fields_with_defs(&other_vault) {
            let id = field.definition().id;
            if field.definition().has_field(&fields::meta::NO_LINK.id) {
                continue;
            }

            if !item.has_field(&id) {
                fields_to_remove.push(id);
            }
        }

        for id in fields_to_remove {
            other_item.remove_field(&id);
        }

        Ok(())
    }

    pub fn update_item_links(
        &self,
        vault: &Vault,
        item: &Item,
    ) -> anyhow::Result<Vec<kind::ItemRef>> {
        let links = item.links()?;
        for kind::ItemRef((other_vault_name, other_path)) in &links {
            self.update_item_link(vault, item, other_vault_name.as_str(), other_path.as_str())?;
        }

        Ok(links.into_iter().map(|l| l.into()).collect())
    }

    pub fn commit_item(
        &self,
        vault: Arc<Vault>,
        item: &Item,
        skip_save: bool,
    ) -> anyhow::Result<()> {
        let link_res = self.update_item_links(&vault, item)?;
        if skip_save {
            return Ok(());
        }

        let other_vault_names: HashSet<_> = link_res
            .into_iter()
            .map(|kind::ItemRef((n, _))| n)
            .collect();
        for other_vault_name in other_vault_names {
            self.save_vault_by_name_deferred(other_vault_name.as_str());
        }
        self.save_vault_deferred(vault);
        Ok(())
    }

    fn remove_link(
        &self,
        field: KnownField<kind::ItemRef>,
        link: kind::ItemRef,
    ) -> anyhow::Result<()> {
        let (other_vault_name, other_path) = link.into();
        let other_vault = self.get_vault(&other_vault_name)?;
        let other_item = other_vault.get_item(Path::new(&other_path.to_string()))?;
        other_item.remove_field(&field.id);
        Ok(())
    }

    fn link_remove_from_list(
        &self,
        field: KnownField<kind::List>,
        link: kind::ItemRef,
        orig_ref: kind::ItemRef,
    ) -> anyhow::Result<()> {
        let (other_vault_name, other_path) = link.into();
        let other_vault = self.get_vault(&other_vault_name)?;
        let other_item = other_vault.get_item(Path::new(&other_path.to_string()))?;
        other_item.remove_value_from_list(field, &orig_ref.into())
    }

    pub fn unlink_item(&self, vault: &Vault, item: &Item) -> anyhow::Result<()> {
        if let Some(link) = item.get_known_field_value(fields::general::LINK)? {
            self.remove_link(fields::general::LINK, link.into()).ok();
        }
        item.remove_field(&fields::general::LINK.id);

        if let Some(link) = item.get_known_field_value(fields::general::ORIGINAL)? {
            self.link_remove_from_list(
                fields::general::DERIVED,
                link.into(),
                vault.itemref_of(item),
            )
            .ok();
        }
        item.remove_field(&fields::general::ORIGINAL.id);

        if let Some(links) = item.get_known_field_value(fields::general::DERIVED)? {
            for link in links {
                self.remove_link(
                    fields::general::ORIGINAL,
                    link.as_itemref()?.to_owned().into(),
                )?;
            }
        }
        item.remove_field(&fields::general::DERIVED.id);

        Ok(())
    }

    pub fn add_dialog(&self, dialog: impl AppModal) {
        let mut l = self.dialog_queue.lock().unwrap();
        l.push(Box::new(dialog));
    }

    pub fn add_global_task(
        &self,
        name: impl Into<String>,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        let mut l = self.task_queue.lock().unwrap();
        l.push(TaskInfo::GlobalTask {
            name: name.into(),
            task_factory: Box::new(task_factory),
        });
    }

    pub fn add_global_message(&self, result: AsyncTaskReturn) {
        let mut l = self.task_queue.lock().unwrap();
        l.push(TaskInfo::GlobalMessage { result });
    }

    pub fn add_task_request(
        &self,
        request_id: egui::Id,
        name: impl Into<String>,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        let mut l = self.task_queue.lock().unwrap();
        l.push(TaskInfo::TaskRequest {
            id: request_id,
            name: name.into(),
            task_factory: Box::new(task_factory),
        });
    }

    #[tracing::instrument]
    pub fn add_completed_task(&self, request_id: egui::Id, result: AsyncTaskReturn) {
        let mut l = self.task_queue.lock().unwrap();
        l.push(TaskInfo::CompletedTaskRequest {
            id: request_id,
            result,
        });
    }

    pub fn catch<T, E: Into<anyhow::Error>, S: Into<String>>(
        &self,
        context: impl FnOnce() -> S,
        f: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, ()> {
        match f() {
            Ok(r) => Ok(r),
            Err(e) => {
                let mut queue = self.error_queue.lock().unwrap();
                queue.push(e.into().context(context().into()));
                Err(())
            }
        }
    }

    pub fn drain_tasks(&self, n: usize) -> Vec<TaskInfo> {
        let mut l = self.task_queue.lock().unwrap();
        let len = l.len();
        l.drain(..n.min(len)).collect()
    }

    pub fn drain_errors(&self) -> Vec<anyhow::Error> {
        self.error_queue.lock().unwrap().drain(..).collect()
    }

    pub fn drain_dialogs(&self) -> Vec<Box<dyn AppModal>> {
        self.dialog_queue.lock().unwrap().drain(..).collect()
    }

    pub fn try_take_request_result(&self, id: egui::Id) -> Option<AsyncTaskReturn> {
        self.results.remove(&id).map(|(_, v)| v)
    }

    pub fn push_request_results(&self, results: Vec<(egui::Id, AsyncTaskReturn)>) {
        for (id, result) in results {
            self.results.insert(id, result);
        }
    }

    pub fn vault_loading(&self) -> bool {
        self.vault_loading.load(Ordering::Relaxed)
    }

    #[tracing::instrument]
    pub fn set_vault_loading(&self) {
        self.vault_loading.store(true, Ordering::Relaxed);
    }

    #[tracing::instrument]
    pub fn reset_vault_loading(&self) {
        self.vault_loading.store(false, Ordering::Relaxed);
    }

    #[tracing::instrument]
    pub fn save_current_vault_deferred(&self) {
        self.set_vault_loading();
        self.add_global_task("Save vault", |state, p| {
            Promise::spawn_async(crate::tasks::vault::save_current_and_linked_vaults(
                state, p,
            ))
        });
    }

    #[tracing::instrument]
    pub fn save_vault_deferred(&self, vault: Arc<Vault>) {
        self.add_global_task(format!("Save {} vault", vault.name), |_, p| {
            Promise::spawn_async(crate::tasks::vault::save_vault(vault, p))
        });
    }

    #[tracing::instrument]
    pub fn save_vault_by_name_deferred(&self, name: &str) {
        let ctx = format!("Save {name} vault");
        let Ok(vault) = self.catch(|| ctx.clone(), || self.get_vault(name)) else {
            return;
        };
        self.save_vault_deferred(vault);
    }

    pub fn filter(&self) -> MutexGuard<'_, FilterExpression> {
        self.filter.lock().unwrap()
    }

    pub fn sorts(&self) -> MutexGuard<'_, Vec<SortExpression>> {
        self.sorts.lock().unwrap()
    }

    pub fn set_filter_and_sorts(&self, filter: FilterExpression, sorts: Vec<SortExpression>) {
        *self.filter.lock().unwrap() = filter;
        *self.sorts.lock().unwrap() = sorts;
    }

    pub fn item_list_ids(&self) -> Vec<ItemId> {
        self.filtered_item_list.item_ids()
    }

    pub fn len_item_list(&self) -> usize {
        self.filtered_item_list.len_items()
    }

    pub fn update_item_list(&self) -> anyhow::Result<bool> {
        let vault = self.current_vault()?;
        let is_new_item_list =
            self.filtered_item_list
                .update(&vault, &self.filter(), &self.sorts())?;
        self.item_list_is_new
            .store(is_new_item_list, Ordering::Relaxed);
        Ok(is_new_item_list)
    }

    pub fn item_list_is_new(&self) -> bool {
        self.item_list_is_new.load(Ordering::Relaxed)
    }

    pub fn update_selection(&self, item_ids: Vec<ItemId>) {
        *self.selected_item_ids.lock().unwrap() = item_ids;
    }

    pub fn len_selected_items(&self) -> usize {
        self.selected_item_ids.lock().unwrap().len()
    }

    pub fn selected_item_ids(&self) -> Vec<ItemId> {
        self.selected_item_ids
            .lock()
            .unwrap()
            .iter()
            .copied()
            .collect()
    }

    pub fn shortcuts(&self) -> Vec<(KeyboardShortcut, ShortcutBehaviour)> {
        self.shortcuts
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    #[tracing::instrument]
    pub fn set_shortcut(&self, shortcut: KeyboardShortcut, behaviour: ShortcutBehaviour) {
        self.shortcuts.lock().unwrap().insert(shortcut, behaviour);
    }

    #[tracing::instrument]
    pub fn set_shortcuts(&self, shortcuts: Vec<(KeyboardShortcut, ShortcutBehaviour)>) {
        let mut l = self.shortcuts.lock().unwrap();
        for (shortcut, behaviour) in shortcuts {
            l.insert(shortcut, behaviour);
        }
    }

    pub fn commit_thumbnail(&self, params: ThumbnailParams, item: ThumbnailCacheItem) {
        if params.height == THUMBNAIL_LOW_QUALITY_HEIGHT {
            self.thumbnail_cache_lq.commit(params.clone(), item.clone());
        }
        self.thumbnail_cache.commit(params, item);
    }

    pub fn resolve_thumbnail(&self, params: &ThumbnailParams) -> ThumbnailCacheItem {
        let mut thumb = ThumbnailCacheItem::Loading;
        if params.height != THUMBNAIL_LOW_QUALITY_HEIGHT {
            thumb = self.thumbnail_cache.read(params.clone());
        }
        if thumb == ThumbnailCacheItem::Loading {
            thumb = self
                .thumbnail_cache_lq
                .read(params.with_height(THUMBNAIL_LOW_QUALITY_HEIGHT));
        }

        thumb
    }

    pub fn drain_thumbnail_requests(&self) -> Vec<ThumbnailParams> {
        let mut requests = self.thumbnail_cache_lq.drain_requests();
        requests.extend(self.thumbnail_cache.drain_requests());
        requests
    }
}

#[derive(Default)]
pub(crate) enum AppStateRef {
    #[default]
    Empty,
    Filled(Arc<AppState>),
}

impl Debug for AppStateRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "AppStateRef::Empty"),
            Self::Filled(_) => write!(f, "AppStateRef::Filled"),
        }
    }
}

impl AppStateRef {
    pub fn new(state: AppState) -> Self {
        Self::Filled(Arc::new(state))
    }

    fn from_inner(inner: Arc<AppState>) -> Self {
        Self::Filled(inner)
    }

    pub fn current_vault_catch(&self) -> Result<Arc<Vault>, ()> {
        self.catch(|| "getting current vault", || self.current_vault())
    }

    pub fn commit_item_catch(
        &self,
        vault: Option<Arc<Vault>>,
        item: &Item,
        skip_save: bool,
    ) -> Result<(), ()> {
        let vault = vault
            .or_else(|| self.current_vault_catch().ok())
            .ok_or(())?;
        self.catch(
            || format!("updating item {}", item.path()),
            || self.commit_item(vault, item, skip_save),
        )
    }
}

impl Deref for AppStateRef {
    type Target = Arc<AppState>;

    fn deref(&self) -> &Self::Target {
        match self {
            AppStateRef::Empty => panic!("Expected filled AppStateRef instead of empty"),
            AppStateRef::Filled(inner) => inner,
        }
    }
}

impl Clone for AppStateRef {
    fn clone(&self) -> Self {
        match self {
            AppStateRef::Empty => AppStateRef::Empty,
            AppStateRef::Filled(inner) => Self::from_inner(Arc::clone(inner)),
        }
    }
}
