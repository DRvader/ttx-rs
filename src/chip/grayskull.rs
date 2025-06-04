use arc::ArcMsgError;
use luwen::ttkmd_if::PciDevice;
use noc_endpoints::NocGrid;
use pci_noc::PciNoc;

use super::noc::{NocAddress, NocInterface};

pub mod arc;
mod noc_endpoints;
mod pci_noc;

pub use arc::ArcMsg;

pub struct Grayskull {
    pub interface: PciNoc,

    pub endpoints: NocGrid,
}

impl Grayskull {
    pub fn init(mut device: PciDevice) -> Result<Self, String> {
        let tlb_index = super::noc::allocate_tlb(&mut device, false).map_err(|v| v.to_string())?;

        let noc = PciNoc {
            device,
            tlb: tlb_index,
        };
        let endpoints = noc_endpoints::get_grid(0);

        let mut gs = Grayskull {
            interface: noc,
            endpoints,
        };

        gs.endpoints =
            noc_endpoints::get_grid(gs.get_harvesting_mask().map_err(|v| v.to_string())?);

        Ok(gs)
    }

    pub fn send_arc_msg(&mut self, msg: arc::ArcMsg) -> Result<arc::ArcMsgOk, ArcMsgError> {
        arc::arc_msg(
            self,
            &msg,
            true,
            std::time::Duration::from_secs(1),
            5,
            3,
            &arc::ArcMsgAddr {
                scratch_base: 0x1ff30060,
                arc_misc_cntl: 0x1ff30100,
            },
        )
    }

    fn get_harvesting_mask(&mut self) -> Result<u32, ArcMsgError> {
        self.send_arc_msg(arc::ArcMsg::GetHarvesting)
            .map(|v| v.arg())
    }
}

impl NocInterface for Grayskull {
    fn noc_read<T: Into<NocAddress>>(
        &mut self,
        noc_id: super::noc::NocId,
        tile: T,
        addr: u64,
        data: &mut [u8],
    ) {
        self.interface
            .tile_read(noc_id, tile.into(), addr, data)
            .unwrap()
    }

    fn noc_read32<T: Into<NocAddress>>(
        &mut self,
        noc_id: super::noc::NocId,
        tile: T,
        addr: u64,
    ) -> u32 {
        self.interface
            .tile_read32(noc_id, tile.into(), addr)
            .unwrap()
    }

    fn noc_write<T: Into<NocAddress>>(
        &mut self,
        noc_id: super::noc::NocId,
        tile: T,
        addr: u64,
        data: &[u8],
    ) {
        self.interface
            .tile_write(noc_id, tile.into(), addr, data)
            .unwrap()
    }

    fn noc_write32<T: Into<NocAddress>>(
        &mut self,
        noc_id: super::noc::NocId,
        tile: T,
        addr: u64,
        value: u32,
    ) {
        self.interface
            .tile_write32(noc_id, tile.into(), addr, value)
            .unwrap()
    }

    fn noc_broadcast(&mut self, noc_id: super::noc::NocId, addr: u64, data: &[u8]) {
        super::noc::noc_multicast(
            &mut self.interface.device,
            &self.interface.tlb,
            luwen::ttkmd_if::tlb::Ordering::STRICT,
            noc_id,
            (0, 0),
            (12, 11),
            addr,
            data,
        )
        .unwrap()
    }

    fn noc_broadcast32(&mut self, noc_id: super::noc::NocId, addr: u64, value: u32) {
        super::noc::noc_multicast32(
            &mut self.interface.device,
            &self.interface.tlb,
            luwen::ttkmd_if::tlb::Ordering::STRICT,
            noc_id,
            (0, 0),
            (12, 11),
            addr,
            value,
        )
        .unwrap()
    }
}
