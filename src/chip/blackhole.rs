use luwen::ttkmd_if::{PciDevice, PciError};
use noc_endpoints::Endpoints;
use pci_noc::PciNoc;
use telemetry::{Telemetry, TelemetryData, TelemetryError};

use super::noc::{NocAddress, NocInterface};

pub mod arc;
mod noc_endpoints;
mod pci_noc;
mod telemetry;

pub struct Blackhole {
    pub interface: PciNoc,

    pub endpoints: Endpoints,

    pub telemetry: Telemetry,
    pub telemetry_cache: Option<TelemetryData>,
}

#[derive(Debug, thiserror::Error)]
pub enum BlackholeError {
    #[error(transparent)]
    PciError(#[from] PciError),

    #[error(transparent)]
    TelemetryError(#[from] TelemetryError),
}

impl Blackhole {
    pub fn init(mut device: PciDevice) -> Result<Self, BlackholeError> {
        let tlb_index = super::noc::allocate_tlb(&mut device, false)?;

        let mut noc = PciNoc {
            device,
            tlb: tlb_index,
        };
        let endpoints = Endpoints::default();

        let mut bh = Blackhole {
            telemetry: Telemetry::new(&mut noc, endpoints.arc.into())?,
            interface: noc,
            endpoints,
            telemetry_cache: None,
        };

        bh.endpoints = Endpoints::new(&mut bh)?;

        Ok(bh)
    }

    pub fn get_telemetry_unchanged(&mut self) -> Result<&TelemetryData, PciError> {
        if self.telemetry_cache.is_none() {
            let temp = Some(
                self.telemetry
                    .read(&mut self.interface, self.endpoints.arc.into())?,
            );
            self.telemetry_cache = temp;
        }
        // Safety: Already confirmed it to be Some above
        unsafe { Ok(self.telemetry_cache.as_ref().unwrap_unchecked()) }
    }

    pub fn send_arc_msg(
        &mut self,
        msg_id: u32,
        data: Option<[u32; 7]>,
    ) -> Result<(u8, u16, [u32; 7]), arc::MessageError> {
        arc::send_arc_msg(self, msg_id, data)
    }
}

impl NocInterface for Blackhole {
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
            self.endpoints.tensix_broadcast[noc_id as u8 as usize].0,
            self.endpoints.tensix_broadcast[noc_id as u8 as usize].1,
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
            self.endpoints.tensix_broadcast[noc_id as u8 as usize].0,
            self.endpoints.tensix_broadcast[noc_id as u8 as usize].1,
            addr,
            value,
        )
        .unwrap()
    }
}
