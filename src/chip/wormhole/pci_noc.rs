use ttkmd_if::{tlb::Ordering, PciDevice, PciError, PossibleTlbAllocation};

use crate::chip::noc::{self, NocAddress, NocId};

pub struct PciNoc {
    pub device: PciDevice,
    pub tlb: PossibleTlbAllocation,
}

impl PciNoc {
    pub fn tile_read(
        &mut self,
        noc_id: NocId,
        tile: NocAddress,
        addr: u64,
        data: &mut [u8],
    ) -> Result<(), PciError> {
        noc::noc_read(
            &mut self.device,
            &self.tlb,
            Ordering::STRICT,
            noc_id,
            tile.get(noc_id).0,
            tile.get(noc_id).1,
            addr,
            data,
        )
    }

    pub fn tile_read32(
        &mut self,
        noc_id: NocId,
        tile: NocAddress,
        addr: u64,
    ) -> Result<u32, PciError> {
        noc::noc_read32(
            &mut self.device,
            &self.tlb,
            Ordering::STRICT,
            noc_id,
            tile.get(noc_id).0,
            tile.get(noc_id).1,
            addr,
        )
    }

    pub fn tile_write(
        &mut self,
        noc_id: NocId,
        tile: NocAddress,
        addr: u64,
        data: &[u8],
    ) -> Result<(), PciError> {
        noc::noc_write(
            &mut self.device,
            &self.tlb,
            Ordering::STRICT,
            noc_id,
            tile.get(noc_id).0,
            tile.get(noc_id).1,
            addr,
            data,
        )
    }

    pub fn tile_write32(
        &mut self,
        noc_id: NocId,
        tile: NocAddress,
        addr: u64,
        value: u32,
    ) -> Result<(), PciError> {
        noc::noc_write32(
            &mut self.device,
            &self.tlb,
            Ordering::STRICT,
            noc_id,
            tile.get(noc_id).0,
            tile.get(noc_id).1,
            addr,
            value,
        )
    }
}
