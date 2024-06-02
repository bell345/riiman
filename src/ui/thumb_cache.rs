use crate::tasks::image::ThumbnailParams;
use chrono::{DateTime, TimeDelta, Utc};
use eframe::egui;
use std::ops::Add;
use std::sync::Mutex;

#[derive(Clone, PartialEq, Eq)]
pub enum ThumbnailCacheItem {
    Loading,
    Loaded(egui::TextureHandle),
}

pub(crate) struct ThumbnailCache {
    cache: moka::sync::Cache<ThumbnailParams, ThumbnailCacheItem>,
    pending_inserts: Mutex<Vec<ThumbnailParams>>,

    is_loading: bool,
    next_load_utc: Option<DateTime<Utc>>,

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

    pub fn read(&self, params: &ThumbnailParams) -> ThumbnailCacheItem {
        self.cache.get_with(params.clone(), || {
            self.pending_inserts.lock().unwrap().push(params.clone());
            ThumbnailCacheItem::Loading
        })
    }

    pub fn commit(&mut self, params: ThumbnailParams, item: ThumbnailCacheItem) {
        self.cache.insert(params, item);
        self.is_loading = false;
    }

    pub fn drain_requests(&mut self) -> Vec<ThumbnailParams> {
        let mut requests = vec![];
        for params in self.pending_inserts.lock().unwrap().drain(..) {
            let conc_blocked = !self.is_concurrent && self.is_loading;
            let time_blocked = self.next_load_utc.unwrap_or(Utc::now()) > Utc::now();

            if conc_blocked || time_blocked {
                self.cache.invalidate(&params);
                continue;
            }

            self.is_loading = true;
            self.next_load_utc = Some(Utc::now().add(self.load_interval));

            requests.push(params);
        }

        requests
    }

    pub fn clear(&mut self) {
        self.cache.invalidate_all();
        self.next_load_utc = None;
    }
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new(u64::MAX, TimeDelta::zero(), true)
    }
}
