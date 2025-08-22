use std::{collections::HashMap, mem::offset_of};

use runtime_shared::{CLaunchData, CoreLaunchData};
use tempfile::TempDir;

use tracing::{debug, info};
use ttx_rs::{
    Arch, Chip,
    chip::noc::{NocId, NocInterface, Tile},
    kernel::KernelData,
    loader::LoadOptions,
    tensix_builder::CacheEnable,
};

use crate::workload::{LoadedWorkload, Workload};

use super::build_kernel_cached;

pub struct PushFirmware {
    pub path: Option<TempDir>,
    pub data: KernelData,
}

#[derive(Clone)]
pub struct PushFirmwareParameters {}

impl PushFirmware {
    pub fn compile(parameters: PushFirmwareParameters, arch: Arch) -> Self {
        let mut files = HashMap::new();

        files.insert(
            "Cargo.toml".to_string(),
            super::common_gen::write_cargo_toml(),
        );
        write_main(&mut files, parameters.clone());

        let link_script = match arch {
            Arch::Grayskull => include_str!("../workload_link/grayskull.x"),
            Arch::Wormhole => include_str!("../workload_link/wormhole.x"),
            Arch::Blackhole => include_str!("../workload_link/blackhole.x"),
            Arch::Unknown(_) => todo!(),
        };

        let (dir, kernel_data) = build_kernel_cached(
            "push",
            arch,
            LoadOptions::new_without_base().use_cache(CacheEnable::CustomDir(
                super::super::SCCACHE_DIR.path().to_path_buf(),
            )),
            Some((link_script.to_string(), vec![])),
            files,
        );

        PushFirmware {
            path: dir,
            data: kernel_data,
        }
    }

    pub fn available_space(&self) -> u64 {
        self.data.bin.data_start.unwrap_or(0)
    }

    pub fn push_workload(&self, device: &mut Chip, workload: Workload) -> LoadedWorkload {
        let mut job_location = self.data["NEXT_JOB_SLOT"];

        let _device = device.dupe().unwrap();
        let tensix = 'outer: loop {
            for tensix in (0..device.tensix_count()).map(|v| _device.tensix(v)) {
                let core_id = device.noc_read32(NocId::Noc1, tensix, self.data["CORE_ID"]);

                let job_launched =
                    device.noc_read32(NocId::Noc1, tensix, self.data["JOB_LAUNCHED"]);
                info!("waiting for job_location to not be 0 [{tensix:?}] {job_launched:x}");
                info!("core id {}", core_id as i32);

                let next_job_addr = device.noc_read32(NocId::Noc1, tensix, job_location);

                if next_job_addr != 0 {
                    // The core is waiting for work
                    info!("[{tensix:?}] job_location {job_location:x}");
                    job_location = next_job_addr as u64;
                    break 'outer tensix;
                }
            }
        };

        let (relocations, mut workload_binary) = workload.get_binary();

        for relocation in relocations {
            let value = match relocation.read_offset {
                ttx_rs::loader::RelocationRead::Base => job_location as u32,
                ttx_rs::loader::RelocationRead::SymbolValue32(value) => job_location as u32 + value,
                ttx_rs::loader::RelocationRead::Offset(read_offset) => {
                    let read_value = &workload_binary[read_offset as usize..];
                    u32::from_le_bytes([read_value[0], read_value[1], read_value[2], read_value[3]])
                }
            };

            let write_loc = &mut workload_binary[relocation.write_offset as usize..];
            let value = ((value as i64 + relocation.addend) as u32).to_le_bytes();

            write_loc[0] = value[0];
            write_loc[1] = value[1];
            write_loc[2] = value[2];
            write_loc[3] = value[3];
        }

        device.noc_write(NocId::Noc1, tensix, job_location, &workload_binary);

        let kernel_launch_data = CLaunchData {
            brisc: CoreLaunchData {
                entry: workload.data["brisc_kmain"] as u32,
                // stack: Some(workload.data["___brisc_stack_top"] as u32),
                stack: None,
            },
            ncrisc: CoreLaunchData {
                entry: workload.data["ncrisc_kmain"] as u32,
                stack: Some(workload.data["___ncrisc_stack_top"] as u32),
            },
            trisc0: CoreLaunchData {
                entry: workload.data["trisc0_kmain"] as u32,
                stack: Some(workload.data["___trisc0_stack_top"] as u32),
            },
            trisc1: CoreLaunchData {
                entry: workload.data["trisc1_kmain"] as u32,
                stack: Some(workload.data["___trisc1_stack_top"] as u32),
            },
            trisc2: CoreLaunchData {
                entry: workload.data["trisc2_kmain"] as u32,
                stack: Some(workload.data["___trisc2_stack_top"] as u32),
            },

            bss: workload.data["_bss"] as u32,
            ebss: workload.data["_ebss"] as u32,

            flag: 1,
        };

        let job_launched = device.noc_read32(NocId::Noc1, tensix, self.data["JOB_LAUNCHED"]);
        info!("[{tensix:?}] {job_launched}");

        let size = std::mem::size_of_val(&kernel_launch_data);
        let data =
            unsafe { std::slice::from_raw_parts(&raw const kernel_launch_data as *const u8, size) };
        device.noc_write(NocId::Noc1, tensix, self.data["JOB_INFO"], data);

        let job_launched = device.noc_read32(NocId::Noc1, tensix, self.data["JOB_LAUNCHED"]);
        info!("[{tensix:?}] {job_launched}");

        LoadedWorkload {
            tile: tensix,
            base_addr: job_location,
            workload,
        }
    }

    pub fn wait(&self, chip: &mut Chip, tile: Tile) {
        let mut old_job_launched = 0;
        loop {
            let job_launched = chip.noc_read32(NocId::Noc1, tile, self.data["JOB_LAUNCHED"]);
            if old_job_launched != job_launched {
                debug!("{:x}", job_launched);
                old_job_launched = job_launched;
            }

            let flag_zero = chip.noc_read32(
                NocId::Noc1,
                tile,
                self.data["JOB_INFO"] + offset_of!(runtime_shared::CLaunchData, flag) as u64,
            ) == 0;
            let job_slot_reset =
                chip.noc_read32(NocId::Noc1, tile, self.data["NEXT_JOB_SLOT"]) != 0;
            if flag_zero && job_slot_reset {
                break;
            }
        }
    }
}

pub fn write_main(files: &mut HashMap<String, String>, parameters: PushFirmwareParameters) {
    let src = super::common_gen::write_main(
        r#"
        #[unsafe(no_mangle)]
        static JOB_INFO: SYNC<runtime_shared::CLaunchData> = SYNC::new(runtime_shared::CLaunchData::cdefault());

        #[unsafe(no_mangle)]
        static mut NEXT_JOB_SLOT: u64 = 0;

        fn wait_for_job() -> (u32, runtime_shared::CLaunchData) {{
            unsafe {{
                *JOB_LAUNCHED.get() = 23;

                let open_slot = (dyn_base() + ((tensix_std::target::noc_map::ALIGNMENT_L1_READ as u32) - 1)) & !((tensix_std::target::noc_map::ALIGNMENT_L1_READ as u32) - 1);
                (&raw mut NEXT_JOB_SLOT).write_volatile(open_slot as u64);

                *JOB_LAUNCHED.get() = 99;

                while JOB_INFO.get().read_volatile().flag == 0 {{}}

                (&raw mut NEXT_JOB_SLOT).write_volatile(0);

                *JOB_LAUNCHED.get() = 4;

                let output = JOB_INFO.get().read_volatile();
                JOB_INFO.get().write_volatile(runtime_shared::CLaunchData::default());

                *JOB_LAUNCHED.get() = 5;

                (open_slot, output)
            }}
        }}
        "#,
        r#"
        *JOB_LAUNCHED.get() = 33;

        let (base_addr, job) = wait_for_job();

        *JOB_LAUNCHED.get() = 100;
        "#,
    );
    files.insert("src/main.rs".to_string(), src);
}
