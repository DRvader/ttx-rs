use std::collections::BTreeMap;

use num_derive::FromPrimitive;
use ttkmd_if::PciError;

use crate::chip::{
    blackhole::arc::{arc_fw_init_status, ArcFwInitStatus},
    noc::{NocAddress, NocId},
};

use super::pci_noc::PciNoc;

#[derive(thiserror::Error, Debug)]
pub enum TelemetryError {
    #[error(transparent)]
    PciError(#[from] PciError),

    #[error("Telemetry is not ready yet")]
    TelemetryNotReady,
}

#[derive(FromPrimitive)]
#[repr(u32)]
pub enum TelemetryTag {
    BoardIdHigh = 1,
    BoardIdLow = 2,
    AsicId = 3,
    HarvestingState = 4,
    UpdateTelemSpeed = 5,
    Vcore = 6,
    Tdp = 7,
    Tdc = 8,
    VddLimits = 9,
    ThmLimits = 10,
    AsicTemperature = 11,
    VregTemperature = 12,
    BoardTemperature = 13,
    Aiclk = 14,
    Axiclk = 15,
    Arcclk = 16,
    L2CPUCLK0 = 17,
    L2CPUCLK1 = 18,
    L2CPUCLK2 = 19,
    L2CPUCLK3 = 20,
    EthLiveStatus = 21,
    DdrStatus = 22,
    DdrSpeed = 23,
    EthFwVersion = 24,
    DdrFwVersion = 25,
    BmAppFwVersion = 26,
    BmBlFwVersion = 27,
    FlashBundleVersion = 28,
    CmFwVersion = 29,
    L2cpuFwVersion = 30,
    FanSpeed = 31,
    TimerHeartbeat = 32,
    TelemEnumCount = 33,
    EnabledTensixCol = 34,
    EnabledEth = 35,
    EnabledGddr = 36,
    EnabledL2Cpu = 37,
    PcieUsage = 38,
    InputCurrent = 39,
    NocTranslation = 40,
    FanRPM = 41,
    Gddr0_1Temp = 42,
    Gddr2_3Temp = 43,
    Gddr4_5Temp = 44,
    Gddr6_7Temp = 45,
    Gddr0_1TempCorrErrs = 46,
    Gddr2_3TempCorrErrs = 47,
    Gddr4_5TempCorrErrs = 48,
    Gddr6_7TempCorrErrs = 49,
    GddrUncorrErrs = 50,
    MaxGddrTemp = 51,
    AsicLocation = 52,
}

#[derive(Default)]
pub struct Telemetry {
    // Key is tag offset is value
    entries: BTreeMap<u16, u16>,

    table_data: u64,
    table_addr: u64,
    max_offset: u64,
}

#[derive(Default)]
pub struct TelemetryData(BTreeMap<u16, u32>);

impl TelemetryData {
    pub fn get(&self, tag: TelemetryTag) -> Option<u32> {
        self.0.get(&(tag as u16)).copied()
    }

    pub fn aiclk(&self) -> Option<u32> {
        self.get(TelemetryTag::Aiclk)
    }

    pub fn translation_enabled(&self) -> bool {
        self.get(TelemetryTag::NocTranslation)
            .map(|v| v != 0)
            .unwrap_or(false)
    }

    pub fn enabled_tensix_columns(&self) -> u32 {
        self.get(TelemetryTag::EnabledTensixCol).unwrap_or(0x3fff)
    }

    pub fn enabled_gddr(&self) -> u32 {
        self.get(TelemetryTag::EnabledGddr).unwrap_or(0xff)
    }

    pub fn enabled_pcie(&self) -> Option<u32> {
        self.get(TelemetryTag::PcieUsage)
    }

    pub fn enabled_ethernet(&self) -> u32 {
        self.get(TelemetryTag::EnabledEth).unwrap_or(0x3fff)
    }
}

impl Telemetry {
    pub fn new(chip: &mut PciNoc, arc: NocAddress) -> Result<Self, TelemetryError> {
        if arc_fw_init_status(chip, arc) != Some(ArcFwInitStatus::Done) {
            return Err(TelemetryError::TelemetryNotReady);
        }

        let telemetry_table_data =
            chip.tile_read32(NocId::Noc0, arc, 0x80030000 + 0x400 + (4 * 12))? as u64;
        let telemetry_table_addr =
            chip.tile_read32(NocId::Noc0, arc, 0x80030000 + 0x400 + (4 * 13))? as u64;

        // Check if the address is within CSM memory. Otherwise, it must be invalid
        if !(0x10000000..=0x1007FFFF).contains(&telemetry_table_addr)
            || !(0x10000000..=0x1007FFFF).contains(&telemetry_table_data)
        {
            println!("{telemetry_table_addr:x} {telemetry_table_data:x}");
            return Err(TelemetryError::TelemetryNotReady);
        }

        let mut output = Telemetry {
            max_offset: 0,
            entries: BTreeMap::new(),
            table_addr: telemetry_table_addr,
            table_data: telemetry_table_data,
        };

        output.reload_entries(chip, arc)?;

        Ok(output)
    }

    pub fn reload_entries(&mut self, chip: &mut PciNoc, arc: NocAddress) -> Result<(), PciError> {
        let entry_count = chip.tile_read32(NocId::Noc0, arc, self.table_addr + 4)?;

        let mut entry_offsets = vec![0; 4 * entry_count as usize];
        chip.tile_read(NocId::Noc0, arc, self.table_addr + 8, &mut entry_offsets)?;

        let mut map = BTreeMap::new();

        for offset in entry_offsets.chunks(4) {
            let offset = u32::from_le_bytes([offset[0], offset[1], offset[2], offset[3]]);

            let tag = (offset & 0xFFFF) as u16;
            let offset = ((offset >> 16) & 0xFFFF) as u16;
            map.insert(tag, offset);
        }

        self.max_offset = map.values().max().copied().map(|v| v + 4).unwrap_or(0) as u64;
        self.entries = map;

        Ok(())
    }

    pub fn read(&self, chip: &mut PciNoc, arc: NocAddress) -> Result<TelemetryData, PciError> {
        let mut data = vec![0; 4 * self.max_offset as usize];
        chip.tile_read(NocId::Noc0, arc, self.table_data, &mut data)?;

        let mut map = BTreeMap::new();
        for (tag, offset) in &self.entries {
            let data = &data[4 * *offset as usize..];
            map.insert(
                *tag,
                u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            );
        }

        Ok(TelemetryData(map))
    }
}
