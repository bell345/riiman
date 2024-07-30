use crate::data::TransformParams;
use chrono::{DateTime, TimeDelta, Utc};
use eframe::egui;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbnailParams {
    pub abs_path: String,
    pub rel_path: String,
    pub last_modified: Option<DateTime<Utc>>,
    pub height: usize,
    pub transform_params: Option<TransformParams>,
}

impl ThumbnailParams {
    pub fn with_height(&self, height: usize) -> Self {
        Self {
            abs_path: self.abs_path.clone(),
            rel_path: self.rel_path.clone(),
            last_modified: self.last_modified,
            height,
            transform_params: self.transform_params.clone(),
        }
    }

    pub fn tex_name(&self) -> String {
        format!("{}_{}", self.abs_path, self.height)
    }

    pub fn hash_path(&self) -> Box<Path> {
        let mut id = format!(
            "{}_{}_{}",
            self.abs_path,
            self.height,
            self.last_modified
                .map_or(String::new(), |dt| dt.to_rfc3339())
        );
        if let Some(params) = self.transform_params.as_ref() {
            id = format!("{}_{}", id, egui::Id::new(params).value());
        }
        let h = base16ct::lower::encode_string(Sha256::digest(id).as_slice());
        // 6f12a101d9[...] -> riiman/6f/12a101d9[...].jpg
        let root_folder = "riiman";
        let folder = &h[..2];
        let file = &format!("{}.jpg", &h[2..]);
        let buf: PathBuf = [root_folder, folder, file].iter().collect();
        buf.into()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ThumbnailCacheItem {
    Loading,
    Loaded(egui::TextureHandle),
}

pub struct ThumbnailCache {
    cache: moka::sync::Cache<ThumbnailParams, ThumbnailCacheItem>,
    pending_inserts: Mutex<Vec<ThumbnailParams>>,

    is_loading: AtomicBool,
    next_load_utc: Mutex<Option<DateTime<Utc>>>,

    load_interval: TimeDelta,
    is_concurrent: bool,
}

impl ThumbnailCache {
    pub fn new(max_cache_size: u64, load_interval: TimeDelta, is_concurrent: bool) -> Self {
        Self {
            cache: moka::sync::CacheBuilder::new(max_cache_size)
                .weigher(|_, v| match v {
                    ThumbnailCacheItem::Loading => 0,
                    ThumbnailCacheItem::Loaded(hndl) => {
                        hndl.byte_size().try_into().unwrap_or(u32::MAX)
                    }
                })
                .build(),
            pending_inserts: Default::default(),
            is_loading: Default::default(),
            next_load_utc: Default::default(),
            load_interval,
            is_concurrent,
        }
    }

    pub fn read(&self, params: ThumbnailParams) -> ThumbnailCacheItem {
        self.cache.get_with(params.clone(), || {
            self.pending_inserts.lock().unwrap().push(params);
            ThumbnailCacheItem::Loading
        })
    }

    pub fn commit(&self, params: ThumbnailParams, item: ThumbnailCacheItem) {
        self.cache.insert(params, item);
        self.is_loading.store(false, Ordering::Relaxed);
    }

    pub fn drain_requests(&self) -> Vec<ThumbnailParams> {
        let mut requests = vec![];
        let mut next_load_utc = self.next_load_utc.lock().unwrap();

        for params in self.pending_inserts.lock().unwrap().drain(..) {
            let conc_blocked = !self.is_concurrent && self.is_loading.load(Ordering::Relaxed);
            let time_blocked = next_load_utc.unwrap_or(Utc::now()) > Utc::now();

            if conc_blocked || time_blocked {
                self.cache.invalidate(&params);
                continue;
            }

            self.is_loading.store(true, Ordering::Relaxed);
            *next_load_utc = Some(Utc::now() + self.load_interval);

            requests.push(params);
        }

        requests
    }

    pub fn clear(&self) {
        self.cache.invalidate_all();
        *self.next_load_utc.lock().unwrap() = None;
    }
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new(u64::MAX, TimeDelta::zero(), true)
    }
}
