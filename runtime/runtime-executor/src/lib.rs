use std::{
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    str::FromStr,
    sync::{LazyLock, Mutex},
};

use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use tracing::warn;
use ttx_rs::tensix_builder::get_target_dir;

pub mod dram_allocator;
pub mod firmware;
pub mod workload;

#[derive(Hash, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum KernelKey {
    Name(String),
    KernelParameters(String, Box<workload::WorkloadBuilder>),
}

#[derive(Serialize, Deserialize)]
pub struct KernelCacheStore(HashMap<KernelKey, String>);

pub struct KernelCache {
    path: PathBuf,
    cache: Mutex<KernelCacheStore>,
}

static SCCACHE_DIR: LazyLock<TempDir> = LazyLock::new(|| TempDir::new().unwrap());

/// To speed up build times cache builds that have identical file systems.
/// The tensix-builder will automatically handle multiple arches and build options so the only thing that matters is the file system contents
static KERNEL_CACHE: LazyLock<KernelCache> = LazyLock::new(|| {
    let target = if let Some(target) = get_target_dir() {
        target.join("tensix-builder/cache/kernel-cache.postcard")
    } else {
        PathBuf::from_str(".cache/ggml-backend-rs/kernel-cache.postcard")
            .expect("could not create a path from constant")
    };

    let mut cache = Mutex::new(KernelCacheStore(HashMap::new()));
    if let Ok(bytes) = std::fs::read(&target) {
        if let Ok(hash) = postcard::from_bytes(&bytes) {
            cache = Mutex::new(hash);
        }
    }

    KernelCache {
        cache,
        path: target,
    }
});

impl KernelCache {
    fn cache_build(&self, key: KernelKey, files: Vec<(String, String)>) -> (String, PathBuf) {
        let name = if let Ok(mut value) = self.cache.lock() {
            value
                .0
                .entry(key.clone())
                .or_insert_with_key(|key| match key {
                    KernelKey::Name(name) => name.clone(),
                    KernelKey::KernelParameters(..) => uuid::Uuid::new_v4().to_string(),
                })
                .clone()
        } else {
            warn!("WORKLOAD cache is poisoned!");

            match key {
                KernelKey::Name(name) => name.clone(),
                KernelKey::KernelParameters(..) => uuid::Uuid::new_v4().to_string(),
            }
        };

        let dir = self
            .path
            .parent()
            .expect("not expecting to be at a root")
            .join("src")
            .join(&name);

        for (path, value) in &files {
            let abs_path = dir.join(path);
            if let Some(dir) = abs_path.parent() {
                if !dir.exists() {
                    std::fs::create_dir_all(dir).expect("to be able to create parent dir");
                }
            }

            let write = if let Ok(existing) = std::fs::read(&abs_path) {
                existing != value.as_bytes()
            } else {
                true
            };

            if write {
                std::fs::write(abs_path, value).expect("to be able to create and write file")
            }
        }

        if let Ok(value) = self.cache.lock() {
            if let Ok(value) = postcard::to_allocvec(&*value) {
                std::fs::write(&self.path, value).ok();
            }
        }

        (name.to_string(), dir)
    }
}
