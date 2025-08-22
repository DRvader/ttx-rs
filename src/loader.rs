use std::{collections::HashMap, path::PathBuf};

use goblin::elf::{program_header, Reloc};
use luwen::luwen_core::Arch;
use serde::{Deserialize, Serialize};
use tensix_builder::{CacheEnable, Rewrite};

use crate::{
    chip::{
        noc::{NocAddress, NocId, NocInterface, Tile},
        Chip,
    },
    kernel::{Alignment16, CoreData, Kernel, KernelBinData, KernelBytes, KernelData},
};

const BRISC_SOFT_RESET: u32 = 1 << 11;
const TRISC_SOFT_RESETS: u32 = (1 << 12) | (1 << 13) | (1 << 14);
const NCRISC_SOFT_RESET: u32 = 1 << 18;

pub fn reset_to_default(device: &mut Chip) {
    device.go_idle();
    device.deassert_riscv_reset();

    // Put tensix back under soft reset
    device.noc_broadcast32(
        NocId::Noc0,
        0xFFB121B0,
        BRISC_SOFT_RESET | TRISC_SOFT_RESETS | NCRISC_SOFT_RESET,
    )
}

pub fn lower_clocks(device: &mut Chip) {
    device.go_idle();
}

pub fn raise_clocks(device: &mut Chip) {
    device.go_busy();
}

pub fn start_all(device: &mut Chip, keep_triscs_under_reset: bool, stagger_start: bool) {
    let staggered_start_enable: u32 = if stagger_start { 1 << 31 } else { 0 };

    let soft_reset_value = if keep_triscs_under_reset {
        NCRISC_SOFT_RESET | TRISC_SOFT_RESETS | staggered_start_enable
    } else {
        NCRISC_SOFT_RESET | staggered_start_enable
    };

    // Take cores out of reset
    device.noc_broadcast32(NocId::Noc0, 0xFFB121B0, soft_reset_value);
}

pub fn stop_all(device: &mut Chip) {
    lower_clocks(device);

    device.noc_broadcast32(
        NocId::Noc0,
        0xFFB121B0,
        BRISC_SOFT_RESET | TRISC_SOFT_RESETS | NCRISC_SOFT_RESET,
    );
}

pub fn start(
    device: &mut Chip,
    core: NocAddress,
    keep_triscs_under_reset: bool,
    stagger_start: bool,
) {
    let staggered_start_enable: u32 = if stagger_start { 1 << 31 } else { 0 };

    let soft_reset_value = if keep_triscs_under_reset {
        NCRISC_SOFT_RESET | TRISC_SOFT_RESETS | staggered_start_enable
    } else {
        NCRISC_SOFT_RESET | staggered_start_enable
    };

    // Take cores out of reset
    device.noc_write32(NocId::Noc0, core, 0xFFB121B0, soft_reset_value);
    let readback = device.noc_read32(NocId::Noc0, core, 0xFFB121B0);
    debug_assert_eq!(
        readback, soft_reset_value,
        "Failed to start core tried to write {soft_reset_value:x} != {readback:x} "
    );
}

pub fn easy_start(device: &mut Chip, core: NocAddress) {
    start(device, core, true, true);
}

pub fn easy_start_all(device: &mut Chip) {
    start_all(device, true, true);
}

pub fn stop<T: Into<NocAddress>>(device: &mut Chip, core: T) {
    let core = core.into();

    let soft_reset_value = BRISC_SOFT_RESET | TRISC_SOFT_RESETS | NCRISC_SOFT_RESET;
    device.noc_write32(NocId::Noc0, core, 0xFFB121B0, soft_reset_value);
    let readback = device.noc_read32(NocId::Noc0, core, 0xFFB121B0);
    debug_assert_eq!(
        readback,
        soft_reset_value,
        "Failed to stop core tried to write {soft_reset_value:x} to {}:{} != {readback:x}",
        core.get(NocId::Noc0).0,
        core.get(NocId::Noc0).1
    );
}

/// The value to substitute on the right side of the
/// relocation operation
#[derive(Clone, Serialize, Deserialize)]
pub enum RelocationRead {
    /// Instead of reading substitute the loaded binary base
    Base,
    /// Perform a relocation based on the symbol value
    /// This implies that the symbol value should have the loaded base
    /// added
    SymbolValue32(u32),
    /// Offset from the loaded base to read from
    Offset(u64),
}

/// We only support loading self contained kernels and no direct communication
/// between the dynamic file and the firmware.
/// We also load all sections as a single block
/// This means that we, in advance, know where symbols where be located relative to a global base
#[derive(Clone, Serialize, Deserialize)]
pub struct KernelRelocation {
    /// The name of the symbol which is being reallocated
    pub name: Option<String>,
    /// The offset relative to the bottom of the binary where the write needs to happen
    pub write_offset: u64,
    /// The offset relative to the bottom of the binary where the read needs to happen
    pub read_offset: RelocationRead,
    /// A constant value to add to the result of the read
    pub addend: i64,
}

fn load_elf(elf: &[u8]) -> KernelData {
    let bin = goblin::elf::Elf::parse(elf).unwrap();

    assert_eq!(bin.entry, 0, "Don't yet support non-zero entrypoint");

    let mut writes = vec![];

    for header in bin.program_headers {
        if header.p_type == program_header::PT_LOAD {
            let write = header.vm_range();
            let data = &elf[header.file_range()];

            writes.push(KernelBytes {
                addr: write.start as u32,
                data: Alignment16(data.to_vec().into_boxed_slice()),
            });
        }
    }

    let mut sym_table = HashMap::with_capacity(bin.syms.len());
    for sym in bin.syms.iter() {
        if let Some(name) = bin.strtab.get_at(sym.st_name) {
            sym_table.insert(name, sym.st_value);
        }
    }

    let mut relocations = Vec::new();

    // The following notation is used to describe relocation computations specific to x86_64 ELF.
    // A: The addend used to compute the value of the relocatable field.
    // B: The base address at which a shared object is loaded into memory during execution. Generally, a shared object file is built with a base virtual address of 0. However, the execution address of the shared object is different.
    // G: The offset into the global offset table at which the address of the relocation entry’s symbol resides during execution.
    // GOT: The address of the global offset table.
    // L: The section offset or address of the procedure linkage table entry for a symbol.
    // P: The section offset or address of the storage unit being relocated, computed using r_offset.
    // S: The value of the symbol whose index resides in the relocation entry.
    // Z: The size of the symbol whose index resides in the relocation entry.
    // Here we use the dynstrtab for resolving the symbols, this is just a subset of the full
    // symbol table. So it's safe to assume that I can perform relocations against my full symbol table
    let mut process_reloc = |reloc: Reloc| {
        let sym = bin
            .dynsyms
            .get(reloc.r_sym)
            .and_then(|v| bin.dynstrtab.get_at(v.st_name).map(|v| v.to_string()));

        match reloc.r_type {
            // Runtime relocation: word32,64 = B + A
            goblin::elf::reloc::R_RISCV_RELATIVE => {
                relocations.push(KernelRelocation {
                    name: sym,
                    write_offset: reloc.r_offset,
                    read_offset: RelocationRead::Base,
                    addend: reloc.r_addend.unwrap_or(0),
                });
            }
            // Runtime relocation: word32 = S + A
            goblin::elf::reloc::R_RISCV_32 => {
                relocations.push(KernelRelocation {
                    name: sym,
                    write_offset: reloc.r_offset,
                    read_offset: RelocationRead::SymbolValue32(
                        bin.dynsyms.get(reloc.r_sym).unwrap().st_value as u32,
                    ),
                    addend: reloc.r_addend.unwrap_or(0),
                });
            }
            ty => unimplemented!("Do not have a handler for the relocation {ty}"),
        }
    };

    for reloc in bin.dynrels.iter() {
        process_reloc(reloc);
    }

    for reloc in bin.dynrelas.iter() {
        process_reloc(reloc);
    }

    for reloc in bin.pltrelocs.iter() {
        process_reloc(reloc);
    }

    let bin_data = KernelBinData {
        start_sync: sym_table.get("START_SYNC").copied(),

        brisc_state: CoreData {
            entry: sym_table.get("__brisc_start").copied(),
            state: sym_table.get("STATE_BRISC").copied(),
            postcode: sym_table.get("POSTCODE_BRISC").copied(),
            panic: sym_table.get("PANIC_DATA_BRISC").copied(),
        },

        ncrisc_state: CoreData {
            entry: sym_table.get("__ncrisc_start").copied(),
            state: sym_table.get("STATE_NCRISC").copied(),
            postcode: sym_table.get("POSTCODE_NCRISC").copied(),
            panic: sym_table.get("PANIC_DATA_NCRISC").copied(),
        },

        trisc0_state: CoreData {
            entry: sym_table.get("__trisc0_start").copied(),
            state: sym_table.get("STATE_TRISC0").copied(),
            postcode: sym_table.get("POSTCODE_TRISC0").copied(),
            panic: sym_table.get("PANIC_DATA_TRISC0").copied(),
        },

        trisc1_state: CoreData {
            entry: sym_table.get("__trisc1_start").copied(),
            state: sym_table.get("STATE_TRISC1").copied(),
            postcode: sym_table.get("POSTCODE_TRISC1").copied(),
            panic: sym_table.get("PANIC_DATA_TRISC1").copied(),
        },

        trisc2_state: CoreData {
            entry: sym_table.get("__trisc2_start").copied(),
            state: sym_table.get("STATE_TRISC2").copied(),
            postcode: sym_table.get("POSTCODE_TRISC2").copied(),
            panic: sym_table.get("PANIC_DATA_TRISC2").copied(),
        },

        data_start: sym_table.get("__firmware_end").copied(),
        unknown_panic: sym_table.get("PANIC_DATA_UNKNOWN").copied(),
        noc_debug: sym_table.get("NOC_DEBUG").copied(),
        core_data_cache: Default::default(),
    };

    KernelData {
        bin: bin_data,
        sym_table: sym_table
            .into_iter()
            .map(|v| (v.0.to_string(), v.1))
            .collect(),
        writes,
        relocations,
    }
}

fn load_to_all(device: &mut Chip, elf: &[u8]) -> KernelData {
    let data = load_elf(elf);

    for write in &data.writes {
        let data = write.data.0.as_ref();
        if data.as_ptr().align_offset(std::mem::align_of::<u32>()) != 0 {
            let layout = std::alloc::Layout::array::<u8>(data.len())
                .unwrap()
                .align_to(std::mem::align_of::<u32>())
                .unwrap();
            let datap = unsafe { std::alloc::alloc(layout) };
            let new_data = unsafe { std::slice::from_raw_parts_mut(datap, data.len()) };
            new_data.copy_from_slice(data);
            device.noc_broadcast(NocId::Noc0, write.addr as u64, new_data);
            unsafe { std::alloc::dealloc(datap, layout) };
        } else {
            device.noc_broadcast(NocId::Noc0, write.addr as u64, data);
        };
    }

    data
}

fn load_to_cores(device: &mut Chip, cores: &[Tile], elf: &[u8]) -> KernelData {
    let data = load_elf(elf);

    for core in cores.iter().copied() {
        data.load(device, NocId::Noc0, core);
    }

    data
}

fn load_to_core(mut device: Chip, noc_id: NocId, core: Tile, elf: &[u8]) -> Kernel {
    let kernel_data = load_to_cores(&mut device, &[core], elf);
    Kernel::new(device, noc_id, core, kernel_data)
}

pub fn load_file_to_all(device: &mut Chip, kernel: PathBuf) -> KernelData {
    let kernel = std::fs::read(kernel).unwrap();
    load_to_all(device, &kernel)
}

pub fn load_file_to_cores(device: &mut Chip, cores: &[Tile], kernel: PathBuf) -> KernelData {
    let kernel = std::fs::read(kernel).unwrap();
    load_to_cores(device, cores, &kernel)
}

pub fn load_file_to_core(device: Chip, noc_id: NocId, core: Tile, kernel: PathBuf) -> Kernel {
    let kernel = std::fs::read(kernel).unwrap();
    load_to_core(device, noc_id, core, &kernel)
}

#[derive(Hash, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOptions {
    pub build_std: bool,
    pub lto: bool,
    pub profile: String,
    pub default_features: bool,
    pub stack_probes: bool,
}

pub struct LoadOptions {
    pub no_wait: bool,
    pub verbose: bool,
    pub use_cache: tensix_builder::CacheEnable,
    pub path: String,
    pub base_path: PathBuf,
    pub hide_output: bool,
    pub noc_id: NocId,

    pub build_options: BuildOptions,
}

impl LoadOptions {
    pub fn new_without_base() -> Self {
        Self {
            hide_output: false,
            verbose: false,
            use_cache: CacheEnable::Disabled,

            base_path: PathBuf::default(),
            path: String::new(),

            no_wait: false,
            noc_id: NocId::Noc0,

            build_options: BuildOptions {
                lto: false,
                build_std: false,
                profile: "release".to_string(),
                default_features: true,
                stack_probes: false,
            },
        }
    }

    pub fn new(base_path: &std::path::Path) -> Self {
        Self {
            hide_output: false,
            verbose: false,
            use_cache: CacheEnable::Disabled,

            base_path: base_path.to_path_buf(),
            path: String::new(),

            no_wait: false,
            noc_id: NocId::Noc0,

            build_options: BuildOptions {
                lto: false,
                build_std: false,
                profile: "release".to_string(),
                default_features: true,
                stack_probes: false,
            },
        }
    }
}

impl LoadOptions {
    pub fn no_wait(mut self, no_wait: bool) -> Self {
        self.no_wait = no_wait;
        self
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn build_std(mut self, build_std: bool) -> Self {
        self.build_options.build_std = build_std;
        self
    }

    pub fn use_cache(mut self, cache: CacheEnable) -> Self {
        self.use_cache = cache;
        self
    }

    pub fn hide_output(mut self) -> Self {
        self.hide_output = true;
        self
    }

    pub fn lto(mut self, lto: bool) -> Self {
        self.build_options.lto = lto;
        self
    }

    pub fn path(mut self, path: &str) -> Self {
        let path = std::path::Path::new(path);
        self.path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_path.join(path).to_path_buf()
        }
        .to_string_lossy()
        .to_string();

        self
    }

    pub fn profile(mut self, profile: &str) -> Self {
        self.build_options.profile = profile.to_string();
        self
    }

    pub fn default_features(mut self, df: bool) -> Self {
        self.build_options.default_features = df;
        self
    }

    pub fn stack_probes(mut self, probe: bool) -> Self {
        self.build_options.stack_probes = probe;
        self
    }

    pub fn noc_id(mut self, noc_id: NocId) -> Self {
        self.noc_id = noc_id;
        self
    }
}

pub fn build_kernel_elf(
    name: &str,
    arch: Arch,
    options: LoadOptions,
    custom_link: Option<(String, Vec<Rewrite>)>,
) -> (KernelData, Vec<u8>) {
    let arch = match arch {
        luwen::luwen_core::Arch::Grayskull => tensix_builder::StandardTarget::Grayskull,
        luwen::luwen_core::Arch::Wormhole => tensix_builder::StandardTarget::Wormhole,
        luwen::luwen_core::Arch::Blackhole => tensix_builder::StandardTarget::Blackhole,
        luwen::luwen_core::Arch::Unknown(_) => todo!(),
    };

    let arch = if let Some((link, rewrites)) = custom_link {
        tensix_builder::TensixTarget::Custom {
            name: format!("{arch}-custom"),
            target_def: tensix_builder::StandardTargetOrCustom::Standard((arch, rewrites)),
            linker_script: link,
        }
    } else {
        tensix_builder::TensixTarget::Standard(arch)
    };

    let profile = match options.build_options.profile.as_str() {
        "debug" => tensix_builder::CargoProfile::Debug,
        "release" => tensix_builder::CargoProfile::Release,
        other => tensix_builder::CargoProfile::Other(other.to_string()),
    };

    let kernel = tensix_builder::invoke_cargo(
        if options.path.is_empty() {
            options.base_path.to_string_lossy().to_string()
        } else {
            options.path
        },
        tensix_builder::CargoOptions {
            target: arch.clone(),
            profile,
            lto: options.build_options.lto,
            use_cache: options.use_cache,
            verbose: options.verbose,
            build_std: options.build_options.build_std,
            default_features: options.build_options.default_features,
            stack_probes: options.build_options.stack_probes,
            kernel_name: name.to_string(),
            hide_output: options.hide_output,
        },
    );

    let elf = std::fs::read(kernel.path).unwrap();
    (load_elf(&elf), elf)
}

pub fn build_kernel(
    name: &str,
    arch: Arch,
    options: LoadOptions,
    custom_link: Option<(String, Vec<Rewrite>)>,
) -> KernelData {
    build_kernel_elf(name, arch, options, custom_link).0
}

pub fn quick_load(name: &str, mut device: Chip, core: Tile, options: LoadOptions) -> Kernel {
    let arch = match device.arch() {
        luwen::luwen_core::Arch::Grayskull => tensix_builder::StandardTarget::Grayskull,
        luwen::luwen_core::Arch::Wormhole => tensix_builder::StandardTarget::Wormhole,
        luwen::luwen_core::Arch::Blackhole => tensix_builder::StandardTarget::Blackhole,
        luwen::luwen_core::Arch::Unknown(_) => todo!(),
    };

    let profile = match options.build_options.profile.as_str() {
        "debug" => tensix_builder::CargoProfile::Debug,
        "release" => tensix_builder::CargoProfile::Release,
        other => tensix_builder::CargoProfile::Other(other.to_string()),
    };

    let build_result = tensix_builder::invoke_cargo(
        if options.path.is_empty() {
            options.base_path.to_string_lossy().to_string()
        } else {
            options.path
        },
        tensix_builder::CargoOptions {
            target: tensix_builder::TensixTarget::Standard(arch.clone()),
            profile,
            lto: options.build_options.lto,
            use_cache: options.use_cache,
            verbose: options.verbose,
            build_std: options.build_options.build_std,
            default_features: options.build_options.default_features,
            stack_probes: options.build_options.stack_probes,
            kernel_name: name.to_string(),
            hide_output: options.hide_output,
        },
    );

    tracing::debug!("{}: stopping {core:?}", device);
    stop(&mut device, core);

    tracing::debug!("{}: deasserting riscv reset", device);
    device.deassert_riscv_reset();

    tracing::debug!("{}: go busy", device);
    device.go_busy();

    tracing::debug!("{}: loading binary", device);

    assert!(build_result.bin, "Can only quick load binary");
    let mut kernel = load_file_to_core(
        device.dupe().unwrap(),
        options.noc_id,
        core,
        build_result.path,
    );

    tracing::debug!("{}: starting {core:?}", device);
    easy_start(&mut device, core.addr);

    tracing::debug!("{}: waiting for {core:?} start", device);
    if kernel.data.bin.start_sync.is_some() {
        kernel.print_state();

        while !kernel.start_sync() {
            if !kernel.all_complete() {
                kernel.print_state_diff();
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    } else {
        tracing::debug!(
            "{}: no start sync point found in elf; not waiting for start",
            device
        );
        kernel.print_state();
    };

    if options.no_wait {
        tracing::debug!("{}: not waiting for kernel to complete", device);
        return kernel;
    }

    tracing::debug!("{}: waiting for kernel to complete", device);
    kernel.wait();

    kernel
}
