use chrono::TimeDelta;
use dashmap::{DashMap, DashSet};
use eframe::egui;
use eframe::egui::KeyboardShortcut;
use indexmap::IndexMap;
use poll_promise::Promise;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::data::{
    kind, FieldStore, FilterExpression, Item, PreviewOptions, ShortcutAction, ThumbnailCache,
    ThumbnailCacheItem, ThumbnailParams, Vault,
};
use crate::errors::AppError;
use crate::fields;
use crate::tasks::sort::{SortDirection, SortExpression};
use crate::tasks::{AsyncTaskReturn, ProgressSenderRef, TaskFactory};
use crate::ui::AppModal;

const THUMBNAIL_CACHE_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const THUMBNAIL_LOAD_INTERVAL_MS: i64 = 50;
const THUMBNAIL_LQ_LOAD_INTERVAL_MS: i64 = 10;
pub const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;

pub(crate) struct AppState {
    task_queue: Mutex<Vec<(String, TaskFactory, bool)>>,
    results: DashMap<String, AsyncTaskReturn>,
    error_queue: Mutex<Vec<anyhow::Error>>,
    dialog_queue: Mutex<Vec<Box<dyn AppModal>>>,
    vaults: DashMap<String, Arc<Vault>>,
    unresolved_vaults: DashSet<String>,
    current_vault_name: Mutex<Option<String>>,
    vault_loading: Mutex<bool>,

    shortcuts: Mutex<IndexMap<KeyboardShortcut, ShortcutAction>>,

    preview: Mutex<PreviewOptions>,

    thumbnail_cache: ThumbnailCache,
    thumbnail_cache_lq: ThumbnailCache,

    filter: Mutex<FilterExpression>,
    sorts: Mutex<Vec<SortExpression>>,
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
            preview: Default::default(),
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
        };

        {
            let mut shortcuts = res.shortcuts.lock().unwrap();
            for shortcut in DEFAULT_SHORTCUTS {
                shortcuts.insert(shortcut, ShortcutAction::None);
            }
        }

        res
    }
}

impl AppState {
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

    pub fn get_vault(&self, name: &str) -> Result<Arc<Vault>, AppError> {
        self.vaults
            .get(name)
            .map(|r| r.clone())
            .ok_or(AppError::VaultDoesNotExist {
                name: name.to_owned(),
            })
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

    fn update_item_link(
        &self,
        vault: &Vault,
        item: &Item,
    ) -> anyhow::Result<Option<kind::ItemRef>> {
        let Some((other_vault_name, other_path)) =
            item.get_known_field_value(fields::general::LINK)?
        else {
            return Ok(None);
        };
        let other_vault = self.get_vault(&other_vault_name)?;

        let other_item = other_vault.get_item(Path::new(&other_path.to_string()))?;

        for field in item.iter_fields_with_defs(vault) {
            if field.definition().has_field(&fields::meta::NO_LINK.id) {
                continue;
            }

            other_vault.set_definition(field.definition().clone());
            other_item.set_field_value(field.definition().id, field.value().clone());
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

        Ok(Some(kind::ItemRef((other_vault_name, other_path))))
    }

    pub fn commit_item(&self, vault: Arc<Vault>, item: &Item) -> anyhow::Result<()> {
        let link_res = self.update_item_link(&vault, &item)?;
        if let Some(kind::ItemRef((other_vault_name, _))) = link_res.as_ref() {
            self.save_vault_by_name(&other_vault_name);
        }
        self.save_vault(vault);
        Ok(())
    }

    fn add_task_impl(
        &self,
        name: String,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
        is_request: bool,
    ) {
        let mut l = self.task_queue.lock().unwrap();
        l.push((name, Box::new(task_factory), is_request));
    }

    pub fn add_dialog(&self, dialog: impl AppModal) {
        let mut l = self.dialog_queue.lock().unwrap();
        l.push(Box::new(dialog));
    }

    pub fn add_task(
        &self,
        name: impl Into<String>,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        self.add_task_impl(name.into(), task_factory, false);
    }

    pub fn add_task_request(
        &self,
        name: impl Into<String>,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        self.add_task_impl(name.into(), task_factory, true);
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

    pub fn drain_tasks(&self, n: usize) -> Vec<(String, TaskFactory, bool)> {
        let mut l = self.task_queue.lock().unwrap();
        let mut v: Vec<_> = vec![];
        for _ in 0..n {
            if let Some(x) = l.pop() {
                v.push(x);
            } else {
                break;
            }
        }

        v
    }

    pub fn drain_errors(&self) -> Vec<anyhow::Error> {
        self.error_queue.lock().unwrap().drain(..).collect()
    }

    pub fn drain_dialogs(&self) -> Vec<Box<dyn AppModal>> {
        self.dialog_queue.lock().unwrap().drain(..).collect()
    }

    pub fn try_take_request_result(&self, name: &str) -> Option<AsyncTaskReturn> {
        self.results.remove(name).map(|(_, v)| v)
    }

    pub fn push_request_results(&self, results: Vec<(String, AsyncTaskReturn)>) {
        for (name, result) in results {
            self.results.insert(name, result);
        }
    }

    pub fn vault_loading(&self) -> bool {
        *self.vault_loading.lock().unwrap()
    }

    pub fn set_vault_loading(&self) {
        *self.vault_loading.lock().unwrap() = true;
    }

    pub fn reset_vault_loading(&self) {
        *self.vault_loading.lock().unwrap() = false;
    }

    pub fn save_current_vault(&self) {
        *self.vault_loading.lock().unwrap() = true;
        self.add_task("Save vault", |state, p| {
            Promise::spawn_async(crate::tasks::vault::save_current_vault(state, p))
        });
    }

    pub fn save_vault(&self, vault: Arc<Vault>) {
        self.add_task(format!("Save {} vault", vault.name), |_, p| {
            Promise::spawn_async(crate::tasks::vault::save_vault(vault, p))
        });
    }

    pub fn save_vault_by_name(&self, name: &str) {
        let ctx = format!("Save {name} vault");
        let Ok(vault) = self.catch(|| ctx.clone(), || self.get_vault(name)) else {
            return;
        };
        self.save_vault(vault);
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

    pub fn shortcuts(&self) -> Vec<(KeyboardShortcut, ShortcutAction)> {
        self.shortcuts
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect()
    }

    pub fn set_shortcut(&self, shortcut: KeyboardShortcut, action: ShortcutAction) {
        self.shortcuts.lock().unwrap().insert(shortcut, action);
    }

    pub fn set_shortcuts(&self, shortcuts: Vec<(KeyboardShortcut, ShortcutAction)>) {
        let mut l = self.shortcuts.lock().unwrap();
        for (shortcut, action) in shortcuts {
            l.insert(shortcut, action);
        }
    }

    pub fn preview_opts(&self) -> PreviewOptions {
        self.preview.lock().unwrap().clone()
    }

    pub fn preview_mut(&self) -> MutexGuard<PreviewOptions> {
        self.preview.lock().unwrap()
    }

    pub fn preview_texture(&self) -> Option<egui::TextureHandle> {
        self.preview.lock().unwrap().texture_handle()
    }

    pub fn set_preview(&self, hndl: egui::TextureHandle) {
        self.preview.lock().unwrap().set_texture(hndl);
    }

    pub fn close_preview(&self) {
        self.preview.lock().unwrap().clear();
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

    pub fn commit_item_catch(&self, vault: Option<Arc<Vault>>, item: &Item) -> Result<(), ()> {
        let vault = vault
            .or_else(|| self.current_vault_catch().ok())
            .ok_or(())?;
        self.catch(
            || format!("updating item {}", item.path()),
            || self.commit_item(vault, item),
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
