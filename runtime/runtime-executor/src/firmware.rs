use std::{collections::HashMap, path::PathBuf};

use ttx_rs::{
    Arch,
    kernel::KernelData,
    loader::{LoadOptions, build_kernel, build_kernel_elf},
    tensix_builder::Rewrite,
};

use crate::KernelKey;

mod common_gen;
pub mod dram_pull;
pub mod push;

pub fn build_kernel_cached(
    name: &str,
    arch: Arch,
    mut options: LoadOptions,
    custom_link: Option<(String, Vec<Rewrite>)>,
    files: HashMap<String, String>,
) -> (Option<tempfile::TempDir>, KernelData) {
    assert_eq!(
        options.base_path,
        PathBuf::default(),
        "cannot perform a cached build with a set base path"
    );

    let (kernel_name, dir) = super::KERNEL_CACHE.cache_build(
        KernelKey::Name(name.to_string()),
        files.into_iter().collect(),
    );

    options.base_path = dir;

    let kernel = build_kernel(&kernel_name, arch, options, custom_link);

    (None, kernel)
}

pub fn build_kernel_elf_cached(
    arch: Arch,
    parameters: super::workload::WorkloadBuilder,
    mut options: LoadOptions,
    custom_link: Option<(String, Vec<Rewrite>)>,
    files: HashMap<String, String>,
) -> (Option<tempfile::TempDir>, (KernelData, Vec<u8>)) {
    assert_eq!(
        options.base_path,
        PathBuf::default(),
        "cannot perform a cached build with a set base path"
    );

    let (kernel_name, dir) = super::KERNEL_CACHE.cache_build(
        KernelKey::KernelParameters(arch.to_string(), Box::new(parameters)),
        files.into_iter().collect(),
    );

    options.base_path = dir;

    let kernel = build_kernel_elf(&kernel_name, arch, options, custom_link);

    (None, kernel)
}
