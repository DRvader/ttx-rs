use std::collections::HashMap;

use runtime_shared::{CbConsumer, CbObserver, CoreLaunchData, LaunchData, LaunchRequest, MaxSize};
use tempfile::TempDir;
use ttx_rs::{
    Arch,
    chip::{
        dma::AlignedDmaBuffer,
        noc::{NocAddress, NocId, NocInterface, Tile},
    },
    kernel::KernelData,
    loader::LoadOptions,
    tensix_builder::CacheEnable,
};

use crate::{
    dram_allocator::MultiBufferAllocator,
    workload::{OutputBuffer, OutputSlot, Workload},
};

use super::build_firmware_cached;

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct DramPullFirmwareParameters {
    pub use_defmt: bool,
}

pub struct DramPullFirmware {
    pub elf: Vec<u8>,
    pub path: Option<TempDir>,
    pub data: KernelData,
    pub dram_allocator: MultiBufferAllocator,
    pub job_request: AlignedDmaBuffer,
}

pub struct BufferRef {
    pub index: usize,
}

pub enum BufferLocation {
    Host(u64),
    Noc { location: NocAddress, addr: u64 },
}

pub struct BufferTracker {
    _remaps: HashMap<usize, BufferLocation>,
}

pub struct Invocation {
    pub inputs: Box<[BufferRef]>,
    pub outputs: Box<[BufferRef]>,
}

pub struct QueuedWorkload {
    pub workload: Workload,
    pub tile: Tile,
    pub offset: u64,
    pub outputs: Box<[OutputBuffer]>,
}

pub struct TensixCbOutput<'a> {
    chip: &'a mut ttx_rs::Chip,
    slot: &'a OutputSlot,
    workload: &'a QueuedWorkload,
}

impl runtime_shared::CbObserverMut for TensixCbOutput<'_> {
    fn get_read_mut(&mut self) -> u32 {
        self.chip.noc_read32(
            NocId::Noc1,
            self.workload.tile,
            self.workload.data(self.slot.symbol_read()),
        )
    }

    fn get_write_mut(&mut self) -> u32 {
        self.chip.noc_read32(
            NocId::Noc1,
            self.workload.tile,
            self.workload.data(self.slot.symbol_write()),
        )
    }

    fn get_capacity_mut(&mut self) -> u32 {
        self.slot.size as u32
    }
}

impl runtime_shared::CbConsumerMut for TensixCbOutput<'_> {
    fn set_read_mut(&mut self, value: u32) {
        self.chip.noc_write32(
            NocId::Noc1,
            self.workload.tile,
            self.workload.data(self.slot.symbol_read()),
            value,
        )
    }

    fn get_data_mut(&mut self, offset: u32, data: &mut [u8]) {
        self.chip.noc_read(
            NocId::Noc1,
            self.workload.tile,
            self.workload.data(self.slot.symbol_data()) + offset as u64,
            data,
        );
    }
}

impl QueuedWorkload {
    pub fn data(&self, name: impl AsRef<str>) -> u64 {
        self.offset + self.workload.data.sym_table[name.as_ref()]
    }

    pub fn pull_output(&self, chip: &mut ttx_rs::Chip, slot: &OutputSlot) -> Box<[u8]> {
        let cb = TensixCbOutput {
            chip,
            slot,
            workload: self,
        };

        let mut data = vec![0; cb.read_size() as usize];
        cb.pop_all(&mut data);
        data.into_boxed_slice()
    }

    pub fn empty_output(&self, chip: &mut ttx_rs::Chip, slot: &OutputSlot) -> Box<[u8]> {
        let mut output = Vec::new();

        let start_time = std::time::Instant::now();
        let mut warned = false;

        // Exit when flushed is true
        loop {
            let flushed =
                chip.noc_read32(NocId::Noc1, self.tile, self.data(slot.symbol_flushed())) != 0;
            output.extend_from_slice(&self.pull_output(chip, slot));

            if !warned && start_time.elapsed() > std::time::Duration::from_secs(1) {
                let read = chip.noc_read32(NocId::Noc1, self.tile, self.data(slot.symbol_read()));
                let write = chip.noc_read32(NocId::Noc1, self.tile, self.data(slot.symbol_write()));

                tracing::warn!(
                    "Looks like we hung waiting for program completion; buffer read {read} - buffer write {write}"
                );
                warned = true;
            }

            if flushed {
                break;
            }
        }

        output.into_boxed_slice()
    }

    pub fn wait_buffers_valid(&self, chip: &mut ttx_rs::Chip) {
        let start_time = std::time::Instant::now();
        let mut warned = false;

        loop {
            let buffers_valid =
                chip.noc_read32(NocId::Noc1, self.tile, self.data("BUFFERS_VALID")) != 0;
            if buffers_valid {
                break;
            }

            if !warned && start_time.elapsed() > std::time::Duration::from_secs(1) {
                tracing::warn!("Looks like we hung waiting for the workload to start");
                warned = true;
            }
        }
    }
}

impl DramPullFirmware {
    pub fn compile(chip: &mut ttx_rs::Chip, parameters: DramPullFirmwareParameters) -> Self {
        let mut files = HashMap::new();

        let need_job_request = chip.alloc_dma_aligned(64 * chip.tensix_count(), 64);
        let job_server = chip.pcie();
        let job_server_addr = chip.pcie_access(need_job_request.physical_address());

        files.insert(
            "Cargo.toml".to_string(),
            super::common_gen::write_cargo_toml(parameters.use_defmt),
        );
        write_main(&mut files, job_server, job_server_addr, parameters.clone());

        let link_script = match chip.arch() {
            Arch::Grayskull => include_str!("../workload_link/grayskull.x"),
            Arch::Wormhole => include_str!("../workload_link/wormhole.x"),
            Arch::Blackhole => include_str!("../workload_link/blackhole.x"),
        };

        let extra_flags = if parameters.use_defmt {
            vec!["-C link-arg=-Tdefmt.x".to_string()]
        } else {
            Vec::new()
        };

        let (dir, (kernel_data, elf)) = build_firmware_cached(
            "dram-pull",
            chip.arch(),
            LoadOptions::new_without_base().use_cache(CacheEnable::CustomDir(
                super::super::SCCACHE_DIR.path().to_path_buf(),
            )),
            Some((link_script.to_string(), vec![])),
            extra_flags,
            files,
        );

        DramPullFirmware {
            path: dir,
            elf,
            data: kernel_data,
            dram_allocator: MultiBufferAllocator::new(vec![
                chip.dram_size() as usize;
                chip.dram_count()
            ]),
            job_request: need_job_request,
        }
    }

    pub fn available_space(&self) -> u64 {
        self.data.bin.data_start.unwrap_or(0)
    }

    pub fn get_workload_binary(&self, workload: &Workload) -> (usize, Box<[u8]>) {
        let (relocations, kernel_binary) = workload.get_binary();
        let kernel_size = kernel_binary.len();

        let mut kernel_relocations = Vec::new();
        kernel_relocations.extend((relocations.len() as u64).to_le_bytes());

        for reloc in relocations {
            let reloc = reloc.to_kernel_relocation(&[&self.data.sym_table]);
            tracing::info!("{:?}", reloc);
            kernel_relocations = postcard::to_extend(&reloc, kernel_relocations).unwrap();
        }

        let mut output = Vec::new();
        output.extend(kernel_binary);
        // Allows these reallocations to be overwritten
        output.extend(kernel_relocations);

        (kernel_size, output.into_boxed_slice())
    }

    pub fn queue_workload(
        &mut self,
        chip: &mut ttx_rs::Chip,
        workload: Workload,
        outputs: Vec<OutputBuffer>,
    ) -> QueuedWorkload {
        let (kernel_size, workload_binary) = self.get_workload_binary(&workload);

        let kernel_allocation = self
            .dram_allocator
            .allocate(workload_binary.len(), 64)
            .expect("Could not allocate dram space for kernel");

        chip.noc_write(
            ttx_rs::chip::noc::NocId::Noc1,
            chip.dram(kernel_allocation.dram_bank)[0],
            kernel_allocation.offset,
            &workload_binary,
        );

        let kernel_launch_data = LaunchData {
            workload_bank: (
                chip.dram(kernel_allocation.dram_bank)[0].addr.n0.0,
                chip.dram(kernel_allocation.dram_bank)[0].addr.n0.1,
            ),
            workload_bank_offset: kernel_allocation.offset,
            workload_bank_size: workload_binary.len() as u64,

            kernel_size: kernel_size as u64,

            brisc: CoreLaunchData {
                entry: workload.data.sym_table["brisc_kmain"] as u32,
                stack: if !chip.arch().is_grayskull() {
                    // For some reason GS has issues with this
                    // Some(workload.data.sym_table["___brisc_stack_top"] as u32)
                    // Now BH has issues?
                    None
                } else {
                    None
                },
            },
            ncrisc: CoreLaunchData {
                entry: workload.data.sym_table["ncrisc_kmain"] as u32,
                stack: Some(workload.data.sym_table["___ncrisc_stack_top"] as u32),
            },
            trisc0: CoreLaunchData {
                entry: workload.data.sym_table["trisc0_kmain"] as u32,
                stack: Some(workload.data.sym_table["___trisc0_stack_top"] as u32),
            },
            trisc1: CoreLaunchData {
                entry: workload.data.sym_table["trisc1_kmain"] as u32,
                stack: Some(workload.data.sym_table["___trisc1_stack_top"] as u32),
            },
            trisc2: CoreLaunchData {
                entry: workload.data.sym_table["trisc2_kmain"] as u32,
                stack: Some(workload.data.sym_table["___trisc2_stack_top"] as u32),
            },

            bss: workload.data.sym_table["_bss"] as u32,
            ebss: workload.data.sym_table["_ebss"] as u32,

            data_bank: (0, 0),
            data_bank_offset: 0,

            flag: 1,
        };

        tracing::debug!("Queuing Job");
        let (loaded_tensix, loaded_offset) = 'inf_loop: loop {
            for queue_addr in 0..chip.tensix_count() {
                let size = ((<Option<LaunchRequest> as MaxSize>::POSTCARD_MAX_SIZE) + 63) & !63;
                let job_ready: Option<LaunchRequest> = unsafe {
                    let base =
                        (self.job_request.mut_ptr() as *const u8).byte_add(queue_addr * size);

                    let slice = std::slice::from_raw_parts(
                        base,
                        <Option<LaunchRequest> as MaxSize>::POSTCARD_MAX_SIZE,
                    );
                    postcard::from_bytes(slice).unwrap()
                };

                if let Some(job_ready) = job_ready {
                    let tensix = chip.tensix(queue_addr);
                    tracing::trace!(
                        "Tensix {tensix:?} is ready for the job; to be loaded at 0x{:x}",
                        job_ready.job_request_addr
                    );
                    chip.noc_write(
                        NocId::Noc1,
                        tensix,
                        job_ready.job_request_addr,
                        &[
                            vec![1u8],
                            postcard::to_allocvec(&kernel_launch_data).unwrap(),
                        ]
                        .concat(),
                    );
                    break 'inf_loop (tensix, job_ready.job_offset);
                }
            }
        };

        let mut value = 0;
        while value != 201 {
            value = chip.noc_read32(NocId::Noc1, loaded_tensix, self.data["JOB_LAUNCHED"]);
            tracing::debug!("{tensix:?}: hi {value:x}", tensix = loaded_tensix);
        }

        self.data
            .bin
            .print_state(chip, NocId::Noc1, loaded_tensix.into());

        QueuedWorkload {
            workload: workload.dupe(),
            tile: loaded_tensix,
            offset: loaded_offset,
            outputs: outputs.into_boxed_slice(),
        }
    }
}

pub fn write_main(
    files: &mut HashMap<String, String>,
    job_server: Tile,
    job_server_addr: u64,
    parameters: DramPullFirmwareParameters,
) {
    let src = super::common_gen::write_main(
        parameters.use_defmt,
        &format!(
            r#"
        use runtime_shared::MaxSize;

        struct JobInfo {{
            entry: u32,
            stack: Option<u32>,
        }}

        const JOB_INFO_SIZE: usize = <Option<runtime_shared::LaunchData> as runtime_shared::MaxSize>::POSTCARD_MAX_SIZE;
        static mut JOB_INFO: [u8; JOB_INFO_SIZE + 1] = [0; JOB_INFO_SIZE + 1];

        static NEXT_JOB_SLOT: SyncUnsafeCell<u64> = SyncUnsafeCell::new(0);

        const LAUNCH_REQUEST_SIZE: usize = <Option<runtime_shared::LaunchRequest> as runtime_shared::MaxSize>::POSTCARD_MAX_SIZE;
        static LAUNCH_REQUEST: SYNC<[u8; LAUNCH_REQUEST_SIZE]> = SYNC::new([0; LAUNCH_REQUEST_SIZE]);

        fn request_job() -> runtime_shared::LaunchData {{
            unsafe {{
                JOB_LAUNCHED.write(23);

                let alignment = tensix_std::target::noc_map::ALIGNMENT_DRAM_READ as u32;
                let open_slot = (dyn_base() + (alignment - 1)) & !(alignment - 1);
                NEXT_JOB_SLOT.write(open_slot as u64);

                let launch_request = Some(runtime_shared::LaunchRequest {{
                    job_request_addr: (&raw const JOB_INFO) as u64,
                    job_offset: NEXT_JOB_SLOT.read(),
                }});

                let to_write = postcard::to_slice(&launch_request, (*LAUNCH_REQUEST.get()).as_mut_slice()).unwrap();
                tensix_std::target::noc::noc_write(
                    tensix_std::target::noc::NocCommandSel::default(),
                    tensix_std::target::noc::NocAddr {{
                        offset: 0x{job_server_addr:x} + (((LAUNCH_REQUEST_SIZE as u64 + 63) & !63) * (CORE_ID.read() as u64)),
                        x_end: {job_server_x},
                        y_end: {job_server_y},
                        ..Default::default()
                    }},
                    to_write,
                    true
                );
                JOB_LAUNCHED.write(3);

                while (&raw mut JOB_INFO[0]).read_volatile() == 0 {{}}

                JOB_LAUNCHED.write(33);

                let output = postcard::from_bytes(&JOB_INFO[1..]).unwrap();
                (&raw mut JOB_INFO[0]).write_volatile(0);

                JOB_LAUNCHED.write(5);

                output
            }}
        }}

        fn load_job(job: &runtime_shared::LaunchData) {{
            unsafe {{
                tensix_std::target::noc::noc_read(
                    tensix_std::target::noc::NocCommandSel::default(),
                    tensix_std::target::noc::NocAddr {{
                        offset: job.workload_bank_offset,
                        x_end: job.workload_bank.0,
                        y_end: job.workload_bank.1,
                        ..Default::default()
                    }},
                    core::slice::from_raw_parts_mut(NEXT_JOB_SLOT.read() as *mut u8, job.workload_bank_size as usize),
                    true
                );
            }}
        }}
    "#,
            job_server_x = job_server.addr.n0.0,
            job_server_y = job_server.addr.n0.1,
            job_server_addr = job_server_addr,
        ),
        r#"
        JOB_LAUNCHED.write(1);

        let job = request_job();

        JOB_LAUNCHED.write(2);

        load_job(&job);

        JOB_LAUNCHED.write(101);

        let binary_addr = NEXT_JOB_SLOT.read() as *mut u8;

        JOB_LAUNCHED.write(102);

        let relocation_addr = (binary_addr as u32) + job.kernel_size as u32;
        let num_relocations = (relocation_addr as *const u64).read_volatile();
        JOB_LAUNCHED.write(num_relocations as u32);
        let relocation_addr = (relocation_addr + core::mem::size_of::<u64>() as u32) as *mut u8;

        JOB_LAUNCHED.write(103);

        let mut relocations = core::slice::from_raw_parts(
            relocation_addr,
            num_relocations as usize * relocate::KernelRelocation::POSTCARD_MAX_SIZE
        );

        let base_addr = binary_addr as u32;

        JOB_LAUNCHED.write(0x104);
    "#,
        r#"
    // Perform relocations
    for i in 0..num_relocations {
        JOB_LAUNCHED.write(0x105 + i as u32);
        let (relocation, next_relocations) = postcard::take_from_bytes::<relocate::KernelRelocation>(relocations).unwrap();
        relocations = next_relocations;

        relocation.relocate_ptr(base_addr as u64, binary_addr);
    }
    "#,
    );
    files.insert("src/main.rs".to_string(), src);
}
