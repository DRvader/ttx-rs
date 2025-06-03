use std::sync::{atomic::AtomicBool, Mutex};

use blackhole::Blackhole;
use grayskull::Grayskull;
use luwen::{luwen_core::Arch, ttkmd_if::PciDevice};
use noc::{NocAddress, NocId, NocInterface, Tile};
use wormhole::Wormhole;

use crate::kernel::{Kernel, KernelData};
pub use crate::loader;

pub mod blackhole;
pub mod dma;
pub mod field;
pub mod grayskull;
pub mod noc;
pub mod wormhole;

pub static ARC_LOCK: Mutex<Vec<Mutex<()>>> = Mutex::new(Vec::new());
pub static IDLE: Mutex<Vec<AtomicBool>> = Mutex::new(Vec::new());

pub enum Chip {
    Grayskull(Grayskull),
    Wormhole(Wormhole),
    Blackhole(Box<Blackhole>),
}

impl std::fmt::Debug for Chip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}[{}]", self.arch(), self.id())
    }
}

impl std::fmt::Display for Chip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}[{}]", self.arch(), self.id())
    }
}

pub fn scan() -> Vec<Result<Chip, String>> {
    let mut devices = Vec::new();
    for id in PciDevice::scan() {
        devices.push(open(id).map_err(|v| v.to_string()));
    }

    devices
}

pub fn open(index: usize) -> Result<Chip, String> {
    let device = PciDevice::open(index).map_err(|v| v.to_string())?;

    device
        .detect_ffffffff_read(None)
        .map_err(|v| v.to_string())?;

    Ok(match device.arch {
        Arch::Grayskull => Chip::Grayskull(Grayskull::init(device).map_err(|v| v.to_string())?),
        Arch::Wormhole => Chip::Wormhole(Wormhole::init(device).map_err(|v| v.to_string())?),
        Arch::Blackhole => Chip::Blackhole(Box::new(
            Blackhole::init(device).map_err(|v| v.to_string())?,
        )),
        Arch::Unknown(id) => {
            unreachable!("Unkown chip type {id:x}");
        }
    })
}

impl Chip {
    pub fn dupe(&mut self) -> Result<Chip, String> {
        Ok(match self {
            Chip::Grayskull(grayskull) => Chip::Grayskull(
                Grayskull::init(
                    PciDevice::open(grayskull.interface.device.id).map_err(|v| v.to_string())?,
                )
                .map_err(|v| v.to_string())?,
            ),
            Chip::Wormhole(wormhole) => Chip::Wormhole(
                Wormhole::init(
                    PciDevice::open(wormhole.interface.device.id).map_err(|v| v.to_string())?,
                )
                .map_err(|v| v.to_string())?,
            ),
            Chip::Blackhole(blackhole) => Chip::Blackhole(Box::new(
                Blackhole::init(
                    PciDevice::open(blackhole.interface.device.id).map_err(|v| v.to_string())?,
                )
                .map_err(|v| v.to_string())?,
            )),
        })
    }

    pub fn arch(&self) -> Arch {
        match self {
            Chip::Grayskull(grayskull) => grayskull.interface.device.arch,
            Chip::Wormhole(wormhole) => wormhole.interface.device.arch,
            Chip::Blackhole(blackhole) => blackhole.interface.device.arch,
        }
    }

    pub fn id(&self) -> usize {
        match self {
            Chip::Grayskull(grayskull) => grayskull.interface.device.id,
            Chip::Wormhole(wormhole) => wormhole.interface.device.id,
            Chip::Blackhole(blackhole) => blackhole.interface.device.id,
        }
    }

    pub fn device(&self) -> &PciDevice {
        match self {
            Chip::Grayskull(grayskull) => &grayskull.interface.device,
            Chip::Wormhole(wormhole) => &wormhole.interface.device,
            Chip::Blackhole(blackhole) => &blackhole.interface.device,
        }
    }

    pub fn device_mut(&mut self) -> &mut PciDevice {
        match self {
            Chip::Grayskull(grayskull) => &mut grayskull.interface.device,
            Chip::Wormhole(wormhole) => &mut wormhole.interface.device,
            Chip::Blackhole(blackhole) => &mut blackhole.interface.device,
        }
    }

    pub fn tensix_count(&self) -> usize {
        match self {
            Chip::Grayskull(grayskull) => grayskull.endpoints.tensix.len(),
            Chip::Wormhole(wormhole) => wormhole.endpoints.tensix.len(),
            Chip::Blackhole(blackhole) => blackhole.endpoints.tensix.len(),
        }
    }

    pub fn tensix(&self, index: usize) -> Tile {
        match self {
            Chip::Grayskull(grayskull) => grayskull.endpoints.tensix[index],
            Chip::Wormhole(wormhole) => wormhole.endpoints.tensix[index],
            Chip::Blackhole(blackhole) => blackhole.endpoints.tensix[index],
        }
    }

    pub fn tensix_l1(&self) -> u64 {
        match self {
            Chip::Grayskull(grayskull) => grayskull.endpoints.tensix_l1_size,
            Chip::Wormhole(wormhole) => wormhole.endpoints.tensix_l1_size,
            Chip::Blackhole(blackhole) => blackhole.endpoints.tensix_l1_size,
        }
    }

    pub fn dram_count(&self) -> usize {
        match self {
            Chip::Grayskull(grayskull) => grayskull.endpoints.dram.len(),
            Chip::Wormhole(wormhole) => wormhole.endpoints.dram.len(),
            Chip::Blackhole(blackhole) => blackhole.endpoints.dram.len(),
        }
    }

    pub fn cores_per_channel(&self) -> usize {
        match self {
            Chip::Grayskull(_grayskull) => 1,
            Chip::Wormhole(_wormhole) => 3,
            Chip::Blackhole(_blackhole) => 3,
        }
    }

    pub fn dram(&self, index: usize) -> &[Tile] {
        match self {
            Chip::Grayskull(grayskull) => &grayskull.endpoints.dram[index..=index],
            Chip::Wormhole(wormhole) => &wormhole.endpoints.dram[index],
            Chip::Blackhole(blackhole) => &blackhole.endpoints.dram[index],
        }
    }

    pub fn dram_size(&self) -> u64 {
        match self {
            Chip::Grayskull(grayskull) => grayskull.endpoints.dram_size,
            Chip::Wormhole(wormhole) => wormhole.endpoints.dram_size,
            Chip::Blackhole(blackhole) => blackhole.endpoints.dram_size,
        }
    }

    pub fn pcie(&self) -> Tile {
        match self {
            Chip::Grayskull(grayskull) => grayskull.endpoints.pci,
            Chip::Wormhole(wormhole) => wormhole.endpoints.pci,
            Chip::Blackhole(blackhole) => blackhole.endpoints.pcie,
        }
    }

    pub fn pcie_access(&self, addr: u64) -> u64 {
        match self {
            Chip::Grayskull(_grayskull) => addr,
            Chip::Wormhole(_wormhole) => 0x8_0000_0000 + addr,
            Chip::Blackhole(_blackhole) => 0x1000_0000_0000_0000 + addr,
        }
    }

    pub fn start(&mut self) {
        let mut idle = if let Ok(idle) = IDLE.lock() {
            idle
        } else {
            return;
        };

        let id = self.id();

        while idle.len() <= id {
            idle.push(AtomicBool::new(true));
        }

        if idle[id].load(std::sync::atomic::Ordering::SeqCst) {
            loader::reset_to_default(self);
            loader::raise_clocks(self);
            idle[id].store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn stop(&mut self, force: bool) {
        let mut idle = if let Ok(idle) = IDLE.lock() {
            idle
        } else {
            return;
        };

        let id = self.id();

        while idle.len() <= id {
            idle.push(AtomicBool::new(true));
        }

        if force || !idle[id].load(std::sync::atomic::Ordering::SeqCst) {
            loader::stop_all(self);
            idle[id].store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn go_idle(&mut self) {
        match self {
            Chip::Grayskull(grayskull) => {
                grayskull
                    .send_arc_msg(grayskull::arc::ArcMsg::SetPowerState(
                        grayskull::arc::PowerState::LongIdle,
                    ))
                    .unwrap();
            }
            Chip::Wormhole(wormhole) => {
                wormhole
                    .send_arc_msg(wormhole::arc::ArcMsg::SetPowerState(
                        wormhole::arc::PowerState::LongIdle,
                    ))
                    .unwrap();
            }
            Chip::Blackhole(blackhole) => {
                blackhole.send_arc_msg(0x54, None).unwrap();
            }
        }
    }

    pub fn go_busy(&mut self) {
        match self {
            Chip::Grayskull(grayskull) => {
                grayskull
                    .send_arc_msg(grayskull::arc::ArcMsg::SetPowerState(
                        grayskull::arc::PowerState::Busy,
                    ))
                    .unwrap();
            }
            Chip::Wormhole(wormhole) => {
                wormhole
                    .send_arc_msg(wormhole::arc::ArcMsg::SetPowerState(
                        wormhole::arc::PowerState::Busy,
                    ))
                    .unwrap();
            }
            Chip::Blackhole(blackhole) => {
                blackhole.send_arc_msg(0x52, None).unwrap();
            }
        }
    }

    pub fn deassert_riscv_reset(&mut self) {
        match self {
            Chip::Grayskull(grayskull) => {
                grayskull
                    .send_arc_msg(grayskull::arc::ArcMsg::DeassertRiscVReset)
                    .unwrap();
            }
            Chip::Wormhole(wormhole) => {
                wormhole
                    .send_arc_msg(wormhole::arc::ArcMsg::DeassertRiscVReset)
                    .unwrap();
            }
            Chip::Blackhole(_blackhole) => {}
        }
    }

    pub fn load(&mut self, name: &str, core: Tile, options: loader::LoadOptions) -> Kernel {
        loader::quick_load(name, self.dupe().unwrap(), core, options)
    }

    pub fn load_kernel(
        &mut self,
        mut data: KernelData,
        noc_id: NocId,
        tile: Tile,
        wait: bool,
    ) -> Kernel {
        self.load_kernels(&mut data, Some(vec![tile]), wait);

        Kernel::new(self.dupe().unwrap(), noc_id, tile, data)
    }

    pub fn load_kernels(&mut self, data: &mut KernelData, tiles: Option<Vec<Tile>>, wait: bool) {
        tracing::debug!("{}[{}]: stopping cores", self.arch(), self.id());

        if let Some(tiles) = &tiles {
            for tile in tiles {
                tracing::trace!("{}[{}]: stopping tile {:?}", self.arch(), self.id(), tile);
                loader::stop(self, *tile);
            }
        } else {
            tracing::trace!("{}[{}]: stopping all tiles", self.arch(), self.id());
            loader::stop_all(self);
        }

        tracing::debug!("{}[{}]: deasserting riscv reset", self.arch(), self.id());
        self.deassert_riscv_reset();

        tracing::debug!("{}[{}]: go busy", self.arch(), self.id());
        self.go_busy();

        tracing::debug!("{}[{}]: loading binary", self.arch(), self.id());

        if let Some(tiles) = &tiles {
            for tile in tiles {
                tracing::trace!(
                    "{}[{}]: loading binary to tile {:?}",
                    self.arch(),
                    self.id(),
                    tile
                );
                data.load(self, noc::NocId::Noc1, *tile);
            }
        } else {
            tracing::trace!(
                "{}[{}]: loading binary to all tensix",
                self.arch(),
                self.id()
            );
            data.load_all(self, noc::NocId::Noc1);
        }

        tracing::debug!("{}[{}]: starting tensix", self.arch(), self.id());

        if let Some(tiles) = &tiles {
            for tile in tiles {
                tracing::trace!("{}[{}]: starting tile {:?}", self.arch(), self.id(), tile);
                loader::start(self, tile.addr, true, true);
            }
        } else {
            tracing::trace!("{}[{}]: starting all tensix", self.arch(), self.id());
            loader::start_all(self, true, true);
        }

        let all_tensix = (0..self.tensix_count()).map(|v| self.tensix(v)).collect();
        let all_tiles = if let Some(tiles) = &tiles {
            tiles
        } else {
            &all_tensix
        };

        if data.bin.start_sync.is_some() {
            tracing::debug!("{}[{}]: waiting for tensix start", self.arch(), self.id());

            for (core_id, tile) in all_tiles.iter().enumerate() {
                tracing::trace!(
                    "{}[{}]: waiting for fw start on {:?}",
                    self.arch(),
                    self.id(),
                    tile
                );

                if let Some(id) = data.sym_table.get("CORE_ID") {
                    self.noc_write32(noc::NocId::Noc1, *tile, *id, core_id as u32);
                }

                data.bin.print_state(self, noc::NocId::Noc1, tile.addr);
                while !data.bin.start_sync(self, noc::NocId::Noc1, tile.addr) {
                    if !data.bin.all_complete(self, noc::NocId::Noc1, tile.addr) {
                        data.bin.print_state_diff(self, noc::NocId::Noc1, tile.addr);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }

                tracing::trace!("{}[{}]: fw started on {:?}", self.arch(), self.id(), tile);
            }
        } else {
            tracing::debug!(
                "{}[{}]: no start sync point found in elf; not waiting for start",
                self.arch(),
                self.id()
            );
            for tile in all_tiles {
                data.bin.print_state(self, noc::NocId::Noc1, tile.addr);
            }
        }

        if !wait {
            tracing::debug!(
                "{}[{}]: not waiting for kernel to complete",
                self.arch(),
                self.id()
            );

            return;
        }

        tracing::debug!(
            "{}[{}]: waiting for kernel to complete",
            self.arch(),
            self.id()
        );
        for tile in all_tiles {
            tracing::trace!(
                "{}[{}]: waiting for kernel to complete on {:?}",
                self.arch(),
                self.id(),
                tile
            );
            data.bin.wait(self, noc::NocId::Noc1, tile.addr);
        }

        self.stop_tile(tiles);
    }

    pub fn stop_tile(&mut self, tiles: Option<Vec<Tile>>) {
        tracing::debug!("{}[{}]: go idle", self.arch(), self.id());
        self.go_idle();

        tracing::debug!("{}[{}]: stopping cores", self.arch(), self.id());

        if let Some(tiles) = &tiles {
            for tile in tiles {
                tracing::trace!("{}[{}]: stopping tile {:?}", self.arch(), self.id(), tile);
                loader::stop(self, *tile);
            }
        } else {
            tracing::trace!("{}[{}]: stopping all tiles", self.arch(), self.id());
            loader::stop_all(self);
        }
    }
}

impl NocInterface for Chip {
    fn noc_read<T: Into<NocAddress>>(
        &mut self,
        noc_id: noc::NocId,
        tile: T,
        addr: u64,
        data: &mut [u8],
    ) {
        match self {
            Chip::Grayskull(grayskull) => grayskull.noc_read(noc_id, tile, addr, data),
            Chip::Wormhole(wormhole) => wormhole.noc_read(noc_id, tile, addr, data),
            Chip::Blackhole(blackhole) => blackhole.noc_read(noc_id, tile, addr, data),
        }
    }

    fn noc_read32<T: Into<NocAddress>>(&mut self, noc_id: noc::NocId, tile: T, addr: u64) -> u32 {
        match self {
            Chip::Grayskull(grayskull) => grayskull.noc_read32(noc_id, tile, addr),
            Chip::Wormhole(wormhole) => wormhole.noc_read32(noc_id, tile, addr),
            Chip::Blackhole(blackhole) => blackhole.noc_read32(noc_id, tile, addr),
        }
    }

    fn noc_write<T: Into<NocAddress>>(
        &mut self,
        noc_id: noc::NocId,
        tile: T,
        addr: u64,
        data: &[u8],
    ) {
        match self {
            Chip::Grayskull(grayskull) => grayskull.noc_write(noc_id, tile, addr, data),
            Chip::Wormhole(wormhole) => wormhole.noc_write(noc_id, tile, addr, data),
            Chip::Blackhole(blackhole) => blackhole.noc_write(noc_id, tile, addr, data),
        }
    }

    fn noc_write32<T: Into<NocAddress>>(
        &mut self,
        noc_id: noc::NocId,
        tile: T,
        addr: u64,
        value: u32,
    ) {
        match self {
            Chip::Grayskull(grayskull) => grayskull.noc_write32(noc_id, tile, addr, value),
            Chip::Wormhole(wormhole) => wormhole.noc_write32(noc_id, tile, addr, value),
            Chip::Blackhole(blackhole) => blackhole.noc_write32(noc_id, tile, addr, value),
        }
    }

    fn noc_broadcast(&mut self, noc_id: noc::NocId, addr: u64, data: &[u8]) {
        match self {
            Chip::Grayskull(grayskull) => grayskull.noc_broadcast(noc_id, addr, data),
            Chip::Wormhole(wormhole) => wormhole.noc_broadcast(noc_id, addr, data),
            Chip::Blackhole(blackhole) => blackhole.noc_broadcast(noc_id, addr, data),
        }
    }

    fn noc_broadcast32(&mut self, noc_id: noc::NocId, addr: u64, value: u32) {
        match self {
            Chip::Grayskull(grayskull) => grayskull.noc_broadcast32(noc_id, addr, value),
            Chip::Wormhole(wormhole) => wormhole.noc_broadcast32(noc_id, addr, value),
            Chip::Blackhole(blackhole) => blackhole.noc_broadcast32(noc_id, addr, value),
        }
    }
}
