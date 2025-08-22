use std::collections::HashMap;

use runtime_shared::{CoreLaunchData, LaunchData, LaunchRequest, MaxSize};
use tempfile::TempDir;
use ttx_rs::{
    Arch,
    chip::noc::{NocId, NocInterface, Tile},
    kernel::KernelData,
    loader::LoadOptions,
    tensix_builder::CacheEnable,
};

use crate::{
    dram_allocator::MultiBufferAllocator,
    workload::{LoadedWorkload, OutputBuffer, Workload},
};

use super::build_kernel_cached;

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct DramPullFirmwareParameters {
    pub job_server: Tile,
    pub job_server_addr: u64,
}

pub struct DramPullFirmware {
    pub path: Option<TempDir>,
    pub data: KernelData,
    pub dram_allocator: MultiBufferAllocator,
}

impl DramPullFirmware {
    pub fn compile(chip: &ttx_rs::Chip, parameters: DramPullFirmwareParameters) -> Self {
        let mut files = HashMap::new();

        files.insert(
            "Cargo.toml".to_string(),
            super::common_gen::write_cargo_toml(),
        );
        write_main(&mut files, parameters.clone());

        let link_script = match chip.arch() {
            Arch::Grayskull => include_str!("../workload_link/grayskull.x"),
            Arch::Wormhole => include_str!("../workload_link/wormhole.x"),
            Arch::Blackhole => include_str!("../workload_link/blackhole.x"),
            Arch::Unknown(_) => todo!(),
        };

        let (dir, kernel_data) = build_kernel_cached(
            "dram-pull",
            chip.arch(),
            LoadOptions::new_without_base().use_cache(CacheEnable::CustomDir(
                super::super::SCCACHE_DIR.path().to_path_buf(),
            )),
            Some((link_script.to_string(), vec![])),
            files,
        );

        DramPullFirmware {
            path: dir,
            data: kernel_data,
            dram_allocator: MultiBufferAllocator::new(vec![
                chip.dram_size() as usize;
                chip.dram_count()
            ]),
        }
    }

    pub fn available_space(&self) -> u64 {
        self.data.bin.data_start.unwrap_or(0)
    }

    pub fn get_workload_binary(&self, workload: &Workload) -> Box<[u8]> {
        let (relocations, kernel_binary) = workload.get_binary();

        let mut kernel_relocations = Vec::new();
        kernel_relocations.extend((relocations.len() as u64).to_le_bytes());
        for reloc in relocations {
            kernel_relocations.extend(reloc.write_offset.to_le_bytes());
            match reloc.read_offset {
                ttx_rs::loader::RelocationRead::Offset(offset) => {
                    kernel_relocations.extend(0u64.to_le_bytes());
                    kernel_relocations.extend(offset.to_le_bytes());
                }
                ttx_rs::loader::RelocationRead::Base => {
                    kernel_relocations.extend(1u64.to_le_bytes());
                    kernel_relocations.extend(0u64.to_le_bytes());
                }
                ttx_rs::loader::RelocationRead::SymbolValue32(value) => {
                    kernel_relocations.extend(2u64.to_le_bytes());
                    kernel_relocations.extend(value.to_le_bytes());
                }
            }
            kernel_relocations.extend(reloc.addend.to_le_bytes());
        }

        [kernel_relocations.into_boxed_slice(), kernel_binary]
            .concat()
            .into_boxed_slice()
    }

    pub fn queue_workload(
        &mut self,
        chip: &mut ttx_rs::Chip,
        workload: Workload,
        buffers: Vec<(Vec<u8>, Vec<u8>)>,
        outputs: Vec<(OutputBuffer, usize)>,
    ) {
        let kernel_binary = self.get_workload_binary(&workload);

        let kernel_allocation = self
            .dram_allocator
            .allocate(kernel_binary.len(), 16)
            .expect("Could not allocate dram space for kernel");

        chip.noc_write(
            ttx_rs::chip::noc::NocId::Noc1,
            chip.dram(kernel_allocation.dram_bank)[0],
            kernel_allocation.offset,
            &kernel_binary,
        );

        let mut kernel_launch_data = LaunchData {
            workload_bank: (
                chip.dram(kernel_allocation.dram_bank)[0].addr.n1.0,
                chip.dram(kernel_allocation.dram_bank)[0].addr.n1.1,
            ),
            workload_bank_offset: kernel_allocation.offset,
            workload_bank_size: kernel_binary.len() as u64,

            brisc: CoreLaunchData {
                entry: workload.data.sym_table["brisc_kmain"] as u32,
                stack: Some(workload.data.sym_table["___brisc_stack_top"] as u32),
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

        let mut output_buffer = Vec::new();
        for buffer in buffers {
            let buffer_0 = self.dram_allocator.allocate(buffer.0.len(), 16).unwrap();
            chip.noc_write(
                NocId::Noc1,
                chip.dram(buffer_0.dram_bank)[0],
                buffer_0.offset,
                &buffer.0,
            );
            let buffer_1 = self.dram_allocator.allocate(buffer.1.len(), 16).unwrap();
            chip.noc_write(
                NocId::Noc1,
                chip.dram(buffer_1.dram_bank)[0],
                buffer_1.offset,
                &buffer.1,
            );

            let mut kernel_data = Vec::new();
            kernel_data.extend_from_slice(&[0, 0, 0, 0]);
            kernel_data.extend_from_slice(&(kernel_data.len() as u32).to_le_bytes());
            kernel_data.extend_from_slice(&(buffer_0.dram_bank as u32).to_le_bytes());
            kernel_data.extend_from_slice(&buffer_0.offset.to_le_bytes());
            kernel_data.extend_from_slice(&(buffer_1.dram_bank as u32).to_le_bytes());
            kernel_data.extend_from_slice(&buffer_1.offset.to_le_bytes());
            {
                let len = kernel_data.len();
                kernel_data[..4].copy_from_slice(&(len as u32).to_le_bytes());
            }

            let data = self.dram_allocator.allocate(kernel_data.len(), 16).unwrap();
            chip.noc_write(
                NocId::Noc1,
                chip.dram(buffer_0.dram_bank)[0],
                buffer_0.offset,
                &kernel_data,
            );

            kernel_launch_data.data_bank = (
                chip.dram(buffer_0.dram_bank)[0].addr.n0.0,
                chip.dram(buffer_0.dram_bank)[0].addr.n0.1,
            );
            kernel_launch_data.data_bank_offset = data.offset;

            let mut need_job_request = chip.alloc_dma_aligned(64 * chip.tensix_count(), 64);

            tracing::debug!("Queuing Job");
            let (loaded_tensix, loaded_offset) = 'inf_loop: loop {
                for queue_addr in 0..chip.tensix_count() {
                    let size = ((<LaunchRequest as MaxSize>::POSTCARD_MAX_SIZE) + 63) & !63;
                    let job_ready: Option<LaunchRequest> = unsafe {
                        let base =
                            (need_job_request.mut_ptr() as *const u8).byte_add(queue_addr * size);

                        let slice = std::slice::from_raw_parts(
                            base,
                            <LaunchRequest as MaxSize>::POSTCARD_MAX_SIZE,
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
                            &postcard::to_allocvec(&kernel_launch_data).unwrap(),
                        );
                        break 'inf_loop (tensix, job_ready.job_offset);
                    }
                }
            };

            let job_state = self.data["JOB_LAUNCHED"];

            tracing::debug!("Waiting for job Complete");
            for (buffer, index) in &outputs {
                if let (Some(completion), Some(completion_addr), Some(completion_count)) = (
                    workload
                        .data
                        .sym_table
                        .get(&buffer.output_completion(*index)),
                    workload.data.sym_table.get(&buffer.output_buffer()),
                    workload.data.sym_table.get(&buffer.output_count()),
                ) {
                    tracing::debug!("Waiting for job to start");
                    let mut known_state = 0;
                    loop {
                        let state = chip.noc_read32(NocId::Noc1, loaded_tensix, job_state);

                        if known_state != state {
                            tracing::trace!("Job state: {state}");
                            known_state = state;
                        }

                        if state >= 200 {
                            break;
                        }
                    }

                    tracing::debug!("Job started, waiting for completion");

                    let mut known_count = 0;
                    loop {
                        let count = chip.noc_read32(
                            NocId::Noc1,
                            loaded_tensix,
                            loaded_offset + *completion_count,
                        );

                        if count != known_count {
                            tracing::trace!("Output count: {count}");
                            known_count = count;
                        }

                        if count >= buffer.size as u32 {
                            tracing::debug!("Job completed {count:x} >= {:x}", buffer.size);
                            break;
                        }
                    }

                    let mut data = vec![0; buffer.size];
                    chip.noc_read(
                        NocId::Noc1,
                        loaded_tensix,
                        loaded_offset + *completion_addr,
                        &mut data,
                    );
                    output_buffer.push(data);

                    chip.noc_write32(NocId::Noc1, loaded_tensix, loaded_offset + *completion, 1);
                }
            }
        }
    }
}

pub fn write_main(files: &mut HashMap<String, String>, parameters: DramPullFirmwareParameters) {
    let src = format!(
        r#"
        #![no_std]
        #![no_main]

        struct JobInfo {{
            entry: u32,
            stack: Option<u32>,
        }}

        static mut JOB_INFO: runtime_shared::LaunchData = runtime_shared::LaunchData::cdefault();
        static NEXT_JOB_SLOT: SyncUnsafeCell<u64> = SyncUnsafeCell::new(0);

        const LAUNCH_REQUEST_SIZE: usize = <Option<runtime_shared::LaunchRequest> as runtime_shared::MaxSize>::POSTCARD_MAX_SIZE;
        static LAUNCH_REQUEST: SYNC<[u8; LAUNCH_REQUEST_SIZE]> = SYNC::new([0; LAUNCH_REQUEST_SIZE]);

        #[repr(align(64))]
        pub struct NocAligned<T>(T);

        impl<T> core::ops::Deref for NocAligned<T> {{
            type Target = T;

            fn deref(&self) -> &<Self as core::ops::Deref>::Target {{
                &self.0
            }}
        }}

        impl<T> core::ops::DerefMut for NocAligned<T> {{
            fn deref_mut(&mut self) -> &mut <Self as core::ops::Deref>::Target {{
                &mut self.0
            }}
        }}

        #[repr(transparent)]
        pub struct SyncUnsafeCell<T>(core::cell::UnsafeCell<T>);
        unsafe impl<T: Sync> Sync for SyncUnsafeCell<T> {{}}

        impl<T> SyncUnsafeCell<T> {{
            pub const fn new(value: T) -> Self {{
                SyncUnsafeCell(core::cell::UnsafeCell::new(value))
            }}

            pub fn get(&self) -> *mut T {{
                self.0.get()
            }}

            pub fn read(&self) -> T {{
                unsafe {{
                    self.get().read_volatile()
                }}
            }}

            pub fn write(&self, value: T) {{
                unsafe {{
                    self.get().write_volatile(value);
                }}
            }}
        }}

        #[repr(transparent)]
        pub struct SyncUnsafeNocCell<T>(NocAligned<SyncUnsafeCell<T>>);

        impl<T> SyncUnsafeNocCell<T> {{
            pub const fn new(value: T) -> Self {{
                SyncUnsafeNocCell(NocAligned(SyncUnsafeCell(core::cell::UnsafeCell::new(value))))
            }}

            pub fn get(&self) -> *mut T {{
                ((self.0).0).0.get()
            }}

            pub fn read(&self) -> T {{
                unsafe {{
                    self.get().read_volatile()
                }}
            }}

            pub fn write(&self, value: T) {{
                unsafe {{
                    self.get().write_volatile(value);
                }}
            }}
        }}

        type SYNC<T> = SyncUnsafeNocCell<T>;

        struct Tile {{
            n0: (u8, u8),
            n1: (u8, u8)
        }}

        impl From<u32> for Tile {{
            fn from(value: u32) -> Self {{
                Tile {{
                    n0: (value as u8, (value >> 8) as u8),
                    n1: ((value >> 16) as u8, (value >> 24) as u8)
                }}
            }}
        }}

        #[unsafe(no_mangle)]
        static mut JOB_LAUNCHED: NocAligned<u32> = NocAligned(0);

        #[unsafe(no_mangle)]
        static CORE_ID: SyncUnsafeCell<NocAligned<i32>> = SyncUnsafeCell::new(NocAligned(-1));

        type SharedCoreLaunchData = Option<(u32, runtime_shared::CoreLaunchData)>;
        static NCRISC_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
        static NCRISC_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);
        static TRISC0_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
        static TRISC0_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);
        static TRISC1_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
        static TRISC1_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);
        static TRISC2_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
        static TRISC2_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);

        use tensix_std::entry;

        fn dyn_base() -> u32 {{
            unsafe extern "Rust" {{
                unsafe static mut __firmware_end: u8;
            }}

            core::ptr::addr_of!(__firmware_end) as u32
        }}

        fn request_job() -> runtime_shared::LaunchData {{
            unsafe {{
                JOB_LAUNCHED.0 = 23;

                let open_slot = (dyn_base() + ((tensix_std::target::noc_map::ALIGNMENT_DRAM_READ as u32) - 1)) & !((tensix_std::target::noc_map::ALIGNMENT_DRAM_READ as u32) - 1);
                *NEXT_JOB_SLOT.get() = open_slot as u64;

                let launch_request = Some(runtime_shared::LaunchRequest {{
                    job_request_addr: (&raw const JOB_INFO) as u64,
                    job_offset: *NEXT_JOB_SLOT.get(),
                }});

                let to_write = postcard::to_slice(&launch_request, (*LAUNCH_REQUEST.get()).as_mut_slice()).unwrap();
                tensix_std::target::noc::noc_write(
                    tensix_std::target::noc::NocCommandSel::default(),
                    tensix_std::target::noc::NocAddr {{
                        offset: 0x{job_server_addr:x} + (LAUNCH_REQUEST_SIZE as u64 * ((*CORE_ID.get()).0 as u64)),
                        x_end: {job_server_x},
                        y_end: {job_server_y},
                        ..Default::default()
                    }},
                    to_write,
                    true
                );
                JOB_LAUNCHED.0 = 3;

                while (&raw mut JOB_INFO).read_volatile().flag == 0 {{}}

                JOB_LAUNCHED.0 = 4;

                let output = (&raw mut JOB_INFO).read_volatile();
                JOB_INFO = runtime_shared::LaunchData::default();

                JOB_LAUNCHED.0 = 5;

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
                    core::slice::from_raw_parts_mut((*NEXT_JOB_SLOT.get()) as *mut u8, job.workload_bank_size as usize),
                    true
                );
            }}
        }}

        fn jump_to_stack(addr: u32, new_sp: u32) {{
            unsafe {{
                let mut old_sp: usize;

                core::arch::asm!(
                    "mv {{0}}, sp",
                    out(reg) old_sp,
                );

                let mut old_sp_ref = &mut old_sp;

                core::arch::asm!(
                    "mv sp, {{0}}",
                    in(reg) new_sp,
                );

                core::mem::transmute::<u32, fn()>(addr)();

                let old_sp = *old_sp_ref;

                core::arch::asm!(
                    "mv {{0}}, sp",
                    in(reg) old_sp,
                );
            }}
        }}

        fn jump_to(base_addr: u32, addr: u32, new_sp: Option<u32>) {{
            if let Some(new_sp) = new_sp {{
                jump_to_stack(base_addr + addr, new_sp)
            }} else {{
                unsafe {{
                    core::mem::transmute::<u32, fn()>(base_addr + addr)()
                }}
            }}
        }}

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {{
            unsafe {{
                let mut i = 0;
                while i < n {{
                    dest.add(i).write(src.add(i).read());
                    i += 1;
                }}
                dest
            }}
        }}

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn memset(dest: *mut u8, src: core::ffi::c_int, n: usize) -> *mut u8 {{
            unsafe {{
                let mut i = 0;
                while i < n {{
                    dest.add(i).write(src as u8);
                    i += 1;
                }}
                dest
            }}
        }}

        #[entry(brisc)]
        unsafe fn brisc_main() -> ! {{
            tensix_std::reset::start_cores();

            unsafe {{
                JOB_LAUNCHED.0 = 0;

                while (*CORE_ID.get()).0 == -1 {{}}
                loop {{
                    JOB_LAUNCHED.0 = 1;

                    let job = request_job();

                    JOB_LAUNCHED.0 = 100;

                    load_job(&job);

                    JOB_LAUNCHED.0 = 101;

                    NCRISC_JOB_RESULT.write(None);
                    TRISC0_JOB_RESULT.write(None);
                    TRISC1_JOB_RESULT.write(None);
                    TRISC2_JOB_RESULT.write(None);

                    let binary_addr = (*NEXT_JOB_SLOT.get()) as *mut u8;
                    let num_relocations = binary_addr.cast::<u32>().read_volatile();
                    let relocation_size = 8 + 8 + 8 + 8;
                    let base_addr = binary_addr.byte_add((num_relocations * relocation_size) as usize) as u32;

                    // Perform relocations
                    for i in 0..num_relocations {{
                        let write_offset = binary_addr.byte_add((i * relocation_size) as usize).cast::<u64>().read_volatile();
                        let ty = binary_addr.byte_add((i * relocation_size + 8) as usize).cast::<u64>().read_volatile();
                        let read_offset = binary_addr.byte_add((i * relocation_size + 8 + 8) as usize).cast::<u64>().read_volatile();
                        let addend = binary_addr.byte_add((i * relocation_size + 8 + 8 + 8) as usize).cast::<i64>().read_volatile();

                        if ty == 0 {{
                            *((base_addr as u64 + write_offset) as *mut u32) = (*((base_addr as u64 + read_offset) as *mut u32) as i64 + addend) as u32;
                        }} else if ty == 1 {{
                            *((base_addr as u64 + write_offset) as *mut u32) = (base_addr as i64 + addend) as u32;
                        }}
                    }}

                    // Zero BSS
                    for addr in job.bss..job.ebss {{
                        *((base_addr + addr) as *mut u32) = 0;
                    }}

                    JOB_LAUNCHED.0 = 200;

                    NCRISC_JOB_POINTER.write(Some((base_addr, job.ncrisc)));
                    TRISC0_JOB_POINTER.write(Some((base_addr, job.trisc0)));
                    TRISC1_JOB_POINTER.write(Some((base_addr, job.trisc1)));
                    TRISC2_JOB_POINTER.write(Some((base_addr, job.trisc2)));

                    jump_to(base_addr, job.brisc.entry, job.brisc.stack);

                    JOB_LAUNCHED.0 = 201;

                    loop {{
                        if NCRISC_JOB_RESULT.read().is_none() {{
                            continue;
                        }}

                        if TRISC0_JOB_RESULT.read().is_none() {{
                            continue;
                        }}

                        if TRISC1_JOB_RESULT.read().is_none() {{
                            continue;
                        }}

                        if TRISC2_JOB_RESULT.read().is_none() {{
                            continue;
                        }}

                        break;
                    }}

                    *JOB_LAUNCHED = 202;
                }}
            }}
        }}

        #[entry(ncrisc)]
        unsafe fn ncrisc_main() -> ! {{
            unsafe {{
                loop {{
                    if let Some((base, info)) = NCRISC_JOB_POINTER.read() {{
                        NCRISC_JOB_POINTER.write(None);
                        jump_to(base, info.entry, info.stack);
                        NCRISC_JOB_RESULT.write(Some(()));
                    }}
                }}
            }}
        }}

        #[entry(trisc0)]
        unsafe fn trisc0_main() -> ! {{
            unsafe {{
                loop {{
                    if let Some((base, info)) = TRISC0_JOB_POINTER.read() {{
                        TRISC0_JOB_POINTER.write(None);
                        jump_to(base, info.entry, info.stack);
                        TRISC0_JOB_RESULT.write(Some(()));
                    }}
                }}
            }}
        }}

        #[entry(trisc1)]
        unsafe fn trisc1_main() -> ! {{
            unsafe {{
                loop {{
                    if let Some((base, info)) = TRISC1_JOB_POINTER.read() {{
                        TRISC1_JOB_POINTER.write(None);
                        jump_to(base, info.entry, info.stack);
                        TRISC1_JOB_RESULT.write(Some(()));
                    }}
                }}
            }}
        }}

        #[entry(trisc2)]
        unsafe fn trisc2_main() -> ! {{
            unsafe {{
                loop {{
                    if let Some((base, info)) = TRISC2_JOB_POINTER.read() {{
                        TRISC2_JOB_POINTER.write(None);
                        jump_to(base, info.entry, info.stack);
                        TRISC2_JOB_RESULT.write(Some(()));
                    }}
                }}
            }}
        }}
        "#,
        job_server_x = parameters.job_server.addr.n0.0,
        job_server_y = parameters.job_server.addr.n0.1,
        job_server_addr = parameters.job_server_addr
    );
    files.insert("src/main.rs".to_string(), src);
}
