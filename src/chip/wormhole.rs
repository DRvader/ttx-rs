use arc::ArcMsgError;
use noc_endpoints::NocGrid;
use pci_noc::PciNoc;
use ttkmd_if::{tlb::Ordering, PciDevice};

use super::noc::{NocAddress, NocInterface};

pub mod arc;
mod noc_endpoints;
mod pci_noc;

pub use arc::ArcMsg;

pub struct Wormhole {
    pub interface: PciNoc,

    pub endpoints: NocGrid,
}

impl Wormhole {
    pub fn init(mut device: PciDevice) -> Result<Self, String> {
        let tlb_index = super::noc::allocate_tlb(&mut device, false).map_err(|v| v.to_string())?;

        let noc = PciNoc {
            device,
            tlb: tlb_index,
        };
        let endpoints = noc_endpoints::get_grid(0);

        let mut wh = Wormhole {
            interface: noc,
            endpoints,
        };

        wh.endpoints =
            noc_endpoints::get_grid(wh.get_harvesting_mask().map_err(|v| v.to_string())?);

        Ok(wh)
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

impl NocInterface for Wormhole {
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
        let (start, end) = match noc_id {
            // ((0, 0), (GRID_SIZE_X - 1, GRID_SIZE_Y - 1))
            // with a few adjustments for issues
            super::noc::NocId::Noc0 => ((1, 0), (9, 11)),
            super::noc::NocId::Noc1 => ((0, 0), (9, 11)),
        };

        super::noc::noc_multicast(
            &mut self.interface.device,
            &self.interface.tlb,
            Ordering::STRICT,
            noc_id,
            start,
            end,
            addr,
            data,
        )
        .unwrap()
    }

    fn noc_broadcast32(&mut self, noc_id: super::noc::NocId, addr: u64, value: u32) {
        let (start, end) = match noc_id {
            // ((0, 0), (GRID_SIZE_X - 1, GRID_SIZE_Y - 1))
            // with a few adjustments for issues
            super::noc::NocId::Noc0 => ((1, 0), (9, 11)),
            super::noc::NocId::Noc1 => ((0, 0), (9, 11)),
        };

        super::noc::noc_multicast32(
            &mut self.interface.device,
            &self.interface.tlb,
            Ordering::STRICT,
            noc_id,
            start,
            end,
            addr,
            value,
        )
        .unwrap()
    }
}
