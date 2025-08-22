use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    chip::noc::{NocAddress, NocId, NocInterface, Tile},
    loader::KernelRelocation,
    Chip,
};

#[derive(Clone, Serialize, Deserialize)]
#[repr(align(16))]
pub struct Alignment16(pub Box<[u8]>);

#[derive(Clone, Serialize, Deserialize)]
pub struct KernelBytes {
    pub addr: u32,
    pub data: Alignment16,
}

impl KernelBytes {
    pub fn len(&self) -> usize {
        self.data.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.0.is_empty()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CoreData {
    pub panic: Option<u64>,
    pub entry: Option<u64>,
    pub state: Option<u64>,
    pub postcode: Option<u64>,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PerCoreCache {
    name: String,
    state: Option<u32>,
    postcode: Option<u32>,
    panic_data: Option<PanicData>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CoreDataCache {
    sync: Option<u32>,
    core_data: Vec<PerCoreCache>,
    panic_data: Option<PanicData>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct KernelBinData {
    pub start_sync: Option<u64>,
    pub brisc_state: CoreData,
    pub ncrisc_state: CoreData,
    pub trisc0_state: CoreData,
    pub trisc1_state: CoreData,
    pub trisc2_state: CoreData,

    pub data_start: Option<u64>,
    pub noc_debug: Option<u64>,
    pub unknown_panic: Option<u64>,

    pub core_data_cache: CoreDataCache,
}

impl KernelBinData {
    pub fn state_vec(&self) -> Vec<(String, CoreData)> {
        vec![
            ("BRISC".to_string(), self.brisc_state.clone()),
            ("NCRISC".to_string(), self.ncrisc_state.clone()),
            ("TRISC0".to_string(), self.trisc0_state.clone()),
            ("TRISC1".to_string(), self.trisc1_state.clone()),
            ("TRISC2".to_string(), self.trisc2_state.clone()),
        ]
    }

    pub fn start_sync(&mut self, chip: &mut Chip, noc_id: NocId, tile: NocAddress) -> bool {
        if self
            .value_vec(chip, noc_id, tile)
            .core_data
            .into_iter()
            .all(|v| v.state.map(|v| v == 0).unwrap_or(true))
        {
            if let Some(sync) = self.start_sync {
                let mut sync_value = chip.noc_read32(noc_id, tile, sync);
                if sync_value != 3 {
                    if sync_value == 1 {
                        chip.noc_write32(noc_id, tile, sync, 2);
                        sync_value = chip.noc_read32(noc_id, tile, sync);
                    }

                    return sync_value == 3;
                }
            }
        }

        // If there isn't a start sync point then we just assume we are g2g
        // If the cores already look started then we just assume we are g2g
        true
    }

    fn print_core_panic_data(
        &mut self,
        chip: &mut Chip,
        noc_id: NocId,
        tile: NocAddress,
        name: &str,
        panic: &PanicData,
    ) -> bool {
        if panic.panicked {
            tracing::error!("{name} Panicked");
            let mut buf = vec![0; panic.filename_len as usize];
            tracing::error!("filename_len: {}", panic.filename_len);
            chip.noc_read(noc_id, tile, panic.filename_addr as u64, &mut buf);

            let filename = String::from_utf8(buf).unwrap();
            tracing::error!("{}; {}", filename, panic.line);

            let mut buf = vec![0; panic.message_len as usize];
            tracing::error!("message_len: {}", panic.message_len);
            chip.noc_read(noc_id, tile, panic.message_addr as u64, &mut buf);

            // TODO(drosen): For some reason the message is not completely valid utf-8
            // Also a long message doesn't seem to work...
            let message = String::from_utf8_lossy(&buf);
            tracing::error!("{}", message);

            return true;
        }

        false
    }

    pub fn read_panic(
        &mut self,
        panic_addr: Option<u64>,
        chip: &mut Chip,
        noc_id: NocId,
        tile: NocAddress,
    ) -> Option<PanicData> {
        // let name = format!("PANIC_DATA_{name}");
        if let Some(postcode_mapping) = panic_addr {
            let mut data = [0; size_of::<PanicData>()];
            chip.noc_read(noc_id, tile, postcode_mapping, &mut data);

            Some(unsafe { std::mem::transmute_copy(&data) })
        } else {
            None
        }
    }

    fn value_vec(&mut self, chip: &mut Chip, noc_id: NocId, tile: NocAddress) -> CoreDataCache {
        let sync = self
            .start_sync
            .map(|sync| chip.noc_read32(noc_id, tile, sync));
        let mut values = Vec::new();
        for state in self.state_vec() {
            let pd = self.read_panic((state.1).panic, chip, noc_id, tile);
            values.push(PerCoreCache {
                name: state.0,
                state: ((state.1).state).map(|v| chip.noc_read32(noc_id, tile, v)),
                postcode: ((state.1).postcode).map(|v| chip.noc_read32(noc_id, tile, v)),
                panic_data: pd,
            });
        }

        CoreDataCache {
            sync,
            core_data: values,
            panic_data: self.read_panic(self.unknown_panic, chip, noc_id, tile),
        }
    }

    pub fn print_state_diff(&mut self, chip: &mut Chip, noc_id: NocId, tile: NocAddress) {
        self.maybe_print_state(chip, noc_id, tile, false);
    }

    pub fn print_state(&mut self, chip: &mut Chip, noc_id: NocId, tile: NocAddress) {
        self.maybe_print_state(chip, noc_id, tile, true);
    }

    pub fn maybe_print_state(
        &mut self,
        chip: &mut Chip,
        noc_id: NocId,
        tile: NocAddress,
        force: bool,
    ) {
        let state = self.value_vec(chip, noc_id, tile);
        if !force
            && (state.sync, &state.core_data)
                == (self.core_data_cache.sync, &self.core_data_cache.core_data)
        {
            return;
        }

        tracing::info!(
            "State for: {}[{}]: {:?}{{{:?}}}",
            chip.arch(),
            chip.id(),
            tile,
            noc_id
        );

        if let Some(sync) = state.sync {
            tracing::info!("SYNC: {}", sync);
            if sync != 3 {
                self.core_data_cache = state;
                return;
            }
        }

        if let Some(postcode_mapping) = self.noc_debug {
            let brc = chip.noc_read32(noc_id, tile, postcode_mapping);
            tracing::info!("noc_debug: 0x{:x}", brc);
        }

        for PerCoreCache {
            name,
            state,
            postcode,
            panic_data,
        } in &state.core_data
        {
            if let Some(panic) = panic_data {
                self.print_core_panic_data(chip, noc_id, tile, name, panic);
            }

            let mut info = format!("{} {{", name);
            let mut prev = false;
            if let Some(state) = state {
                info = format!("{info} STATE: {state}");
                prev = true;
            }
            if let Some(postcode) = postcode {
                #[allow(unused_assignments)]
                if prev {
                    info = format!("{info},");
                    prev = false;
                }
                info = format!("{info} POSTCODE: {postcode:x}");
            }
            info = format!("{info} }}");

            tracing::info!(info);
        }
        if let Some(panic) = &state.panic_data {
            self.print_core_panic_data(chip, noc_id, tile, "UNKNOWN", panic);
        }

        self.core_data_cache = state;
    }

    pub fn check_panic(&mut self, chip: &mut Chip, noc_id: NocId, tile: NocAddress) -> bool {
        let value = self.value_vec(chip, noc_id, tile);
        value
            .core_data
            .iter()
            .any(|v| v.state.map(|v| v == 6).unwrap_or(false))
    }

    pub fn wait(&mut self, chip: &mut Chip, noc_id: NocId, tile: NocAddress) {
        while !self.all_complete(chip, noc_id, tile) {
            std::thread::sleep(std::time::Duration::from_millis(10));
            self.print_state_diff(chip, noc_id, tile);
        }

        crate::loader::stop(chip, tile);

        self.print_state_diff(chip, noc_id, tile)
    }

    /// Marked by all cores either having completed... or not started
    pub fn all_complete(&mut self, chip: &mut Chip, noc_id: NocId, tile: NocAddress) -> bool {
        let states = self.state_vec();

        let state_value = states
            .iter()
            .filter_map(|v| v.1.state)
            .map(|v| chip.noc_read32(noc_id, tile, v))
            .collect::<Vec<_>>();

        let total_count = state_value.len();
        let complete_count = state_value.iter().filter(|v| **v >= 3).count();
        let not_started_count = state_value.iter().filter(|v| **v == 0).count();

        total_count == 0
            || (complete_count > 0 && complete_count + not_started_count == states.len())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct KernelData {
    pub sym_table: HashMap<String, u64>,
    pub writes: Vec<KernelBytes>,
    pub bin: KernelBinData,
    pub relocations: Vec<KernelRelocation>,
}

impl<S: AsRef<str>> std::ops::Index<S> for KernelData {
    type Output = u64;

    fn index(&self, index: S) -> &Self::Output {
        &self.sym_table[index.as_ref()]
    }
}

impl KernelData {
    pub fn load<T: Into<NocAddress>>(&self, chip: &mut Chip, noc_id: NocId, tile: T) {
        let tile = tile.into();

        for write in &self.writes {
            let data = write.data.0.as_ref();
            // for i in 0..data.len() / 4 {
            // let data = u32::from_le_bytes([
            // data[i * 4],
            // data[i * 4 + 1],
            // data[i * 4 + 2],
            // data[i * 4 + 3],
            // ]);
            // device
            // .noc_write32(0, x, y, write.addr as u64 + i as u64 * 4, data)
            // .unwrap();
            // }

            if data.as_ptr().align_offset(std::mem::align_of::<u32>()) != 0 {
                let layout = std::alloc::Layout::array::<u8>(data.len())
                    .unwrap()
                    .align_to(std::mem::align_of::<u32>())
                    .unwrap();
                let datap = unsafe { std::alloc::alloc(layout) };
                let new_data = unsafe { std::slice::from_raw_parts_mut(datap, data.len()) };
                new_data.copy_from_slice(data);
                chip.noc_write(noc_id, tile, write.addr as u64, new_data);
                unsafe { std::alloc::dealloc(datap, layout) };
            } else {
                chip.noc_write(noc_id, tile, write.addr as u64, data);
            };

            // Readback
            let mut readback_data = vec![0; write.data.0.len()];
            chip.noc_read(noc_id, tile, write.addr as u64, &mut readback_data);
            debug_assert_eq!(readback_data.as_slice(), write.data.0.as_ref());
        }

        for relocation in &self.relocations {
            let value = match relocation.read_offset {
                crate::loader::RelocationRead::Base => 0,
                // The loaded offset is 0
                crate::loader::RelocationRead::SymbolValue32(value) => value,
                crate::loader::RelocationRead::Offset(offset) => {
                    chip.noc_read32(noc_id, tile, offset)
                }
            };
            chip.noc_write32(
                noc_id,
                tile,
                relocation.write_offset,
                (value as i64 + relocation.addend) as u32,
            );
        }
    }

    pub fn load_all(&self, chip: &mut Chip, noc_id: NocId) {
        for write in &self.writes {
            let data = write.data.0.as_ref();
            if data.as_ptr().align_offset(std::mem::align_of::<u32>()) != 0 {
                let layout = std::alloc::Layout::array::<u8>(data.len())
                    .unwrap()
                    .align_to(std::mem::align_of::<u32>())
                    .unwrap();
                let datap = unsafe { std::alloc::alloc(layout) };
                let new_data = unsafe { std::slice::from_raw_parts_mut(datap, data.len()) };
                new_data.copy_from_slice(data);
                chip.noc_broadcast(noc_id, write.addr as u64, new_data);
                unsafe { std::alloc::dealloc(datap, layout) };
            } else {
                chip.noc_broadcast(noc_id, write.addr as u64, data);
            };

            // Readback
            for tensix in 0..chip.tensix_count() {
                let mut readback_data = vec![0; write.data.0.len()];
                chip.noc_read(
                    noc_id,
                    chip.tensix(tensix),
                    write.addr as u64,
                    &mut readback_data,
                );
                debug_assert_eq!(readback_data.as_slice(), write.data.0.as_ref());
            }
        }

        for relocation in &self.relocations {
            let value = match relocation.read_offset {
                crate::loader::RelocationRead::Base => 0,
                // The loaded offset is 0
                crate::loader::RelocationRead::SymbolValue32(value) => value,
                crate::loader::RelocationRead::Offset(offset) => {
                    chip.noc_read32(noc_id, chip.tensix(0), offset)
                }
            };
            chip.noc_broadcast32(
                noc_id,
                relocation.write_offset,
                (value as i64 + relocation.addend) as u32,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_entry(
        &mut self,
        chip: &mut Chip,
        noc_id: NocId,
        core: NocAddress,
        ncrisc: Option<u64>,
        trisc0: Option<u64>,
        trisc1: Option<u64>,
        trisc2: Option<u64>,
    ) {
        pub const TENSIX_CFG_BASE: u32 = 4293853184;

        pub const TRISC0_RESET_PC_ADDR: u32 = 158;
        pub const TRISC1_RESET_PC_ADDR: u32 = 159;
        pub const TRISC2_RESET_PC_ADDR: u32 = 160;
        pub const TRISC_RESET_PC_OVERRIDE_EN: u32 = 161;
        pub const NCRISC_RESET_PC_ADDR: u32 = 162;
        pub const NCRISC_RESET_PC_OVERRIDE_EN: u32 = 163;

        if let Some(trisc0) = trisc0 {
            chip.noc_write32(
                noc_id,
                core,
                (TENSIX_CFG_BASE + TRISC0_RESET_PC_ADDR) as u64,
                trisc0 as u32,
            );
        }
        if let Some(trisc1) = trisc1 {
            chip.noc_write32(
                noc_id,
                core,
                (TENSIX_CFG_BASE + TRISC1_RESET_PC_ADDR) as u64,
                trisc1 as u32,
            );
        }
        if let Some(trisc2) = trisc2 {
            chip.noc_write32(
                noc_id,
                core,
                (TENSIX_CFG_BASE + TRISC2_RESET_PC_ADDR) as u64,
                trisc2 as u32,
            );
        }
        chip.noc_write32(
            noc_id,
            core,
            (TENSIX_CFG_BASE + TRISC_RESET_PC_OVERRIDE_EN) as u64,
            if trisc0.is_some() { 1 } else { 0 }
                | if trisc1.is_some() { 0b10 } else { 0 }
                | if trisc2.is_some() { 0b100 } else { 0 },
        );

        if let Some(ncrisc) = ncrisc {
            chip.noc_write32(
                noc_id,
                core,
                (TENSIX_CFG_BASE + NCRISC_RESET_PC_ADDR) as u64,
                ncrisc as u32,
            );
            chip.noc_write32(
                noc_id,
                core,
                (TENSIX_CFG_BASE + NCRISC_RESET_PC_OVERRIDE_EN) as u64,
                1,
            );
        }
    }
}

pub struct Kernel {
    pub device: Chip,
    pub noc_id: NocId,
    pub core: Tile,
    pub data: KernelData,
}

impl<S: AsRef<str>> std::ops::Index<S> for Kernel {
    type Output = u64;

    fn index(&self, index: S) -> &Self::Output {
        &self.data[index]
    }
}

impl Kernel {
    pub fn new(device: Chip, noc_id: NocId, core: Tile, data: KernelData) -> Self {
        Self {
            device,
            noc_id,
            core,
            data,
        }
    }
}

// TODO(drosen): This should be a shared definition
#[repr(C)]
#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct PanicData {
    pub filename_addr: u32,
    pub filename_len: u32,
    pub line: u32,

    pub message_addr: u32,
    pub message_len: u32,

    pub stack_pointer: u32,
    pub program_counter: u32,

    pub panicked: bool,
}

impl Kernel {
    pub fn start_sync(&mut self) -> bool {
        self.data
            .bin
            .start_sync(&mut self.device, self.noc_id, self.core.addr)
    }

    pub fn print_state_diff(&mut self) {
        self.maybe_print_state(false);
    }

    pub fn print_state(&mut self) {
        self.maybe_print_state(true);
    }

    pub fn maybe_print_state(&mut self, force: bool) {
        self.data
            .bin
            .maybe_print_state(&mut self.device, self.noc_id, self.core.addr, force)
    }

    pub fn check_panic(&mut self) -> bool {
        self.data
            .bin
            .check_panic(&mut self.device, self.noc_id, self.core.addr)
    }

    pub fn wait_id(&mut self, noc_id: NocId) {
        self.data.bin.wait(&mut self.device, noc_id, self.core.addr);
    }

    pub fn wait(&mut self) {
        self.wait_id(self.noc_id);
    }

    /// Marked by all cores either having completed... or not started
    pub fn all_complete(&mut self) -> bool {
        self.data
            .bin
            .all_complete(&mut self.device, self.noc_id, self.core.addr)
    }

    pub fn set_entry(&mut self) {
        let cores = (
            self.data.bin.ncrisc_state.entry,
            self.data.bin.trisc0_state.entry,
            self.data.bin.trisc1_state.entry,
            self.data.bin.trisc2_state.entry,
        );

        self.data.set_entry(
            &mut self.device,
            self.noc_id,
            self.core.addr,
            cores.0,
            cores.1,
            cores.2,
            cores.3,
        )
    }

    pub fn read_id(&mut self, noc_id: NocId, addr: u64, data: &mut [u8]) {
        self.device.noc_read(noc_id, self.core, addr, data);
    }

    pub fn write_id(&mut self, noc_id: NocId, addr: u64, data: &[u8]) {
        self.device.noc_write(noc_id, self.core, addr, data);
    }

    pub fn read32_id(&mut self, noc_id: NocId, addr: u64) -> u32 {
        self.device.noc_read32(noc_id, self.core, addr)
    }

    pub fn write32_id(&mut self, noc_id: NocId, addr: u64, value: u32) {
        self.device.noc_write32(noc_id, self.core, addr, value);
    }

    pub fn read(&mut self, addr: u64, data: &mut [u8]) {
        self.read_id(self.noc_id, addr, data);
    }

    pub fn write(&mut self, addr: u64, data: &[u8]) {
        self.write_id(self.noc_id, addr, data);
    }

    pub fn read32(&mut self, addr: u64) -> u32 {
        self.read32_id(self.noc_id, addr)
    }

    pub fn write32(&mut self, addr: u64, value: u32) {
        self.write32_id(self.noc_id, addr, value)
    }
}
