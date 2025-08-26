use ttkmd_if::PciError;

use crate::chip::noc::{NocAddress, Tile};

use super::Blackhole;

const _PHYS_TO_NOC0_X: &[u32] = &[0, 1, 16, 2, 15, 3, 14, 4, 13, 5, 12, 6, 11, 7, 10, 8, 9];
const _PHYS_TO_NOC0_Y: &[u32] = &[0, 1, 11, 2, 10, 3, 9, 4, 8, 5, 7, 6];

const GRID_SIZE_X: u8 = 17;
const GRID_SIZE_Y: u8 = 12;

const _NUM_TENSIX_ROWS: u32 = 10;
const _NUM_TENSIX_COLS: u32 = 14;

const GDDR_NOC0_COORDS: &[[(u8, u8); 3]] = &[
    [(0, 0), (0, 1), (0, 11)],
    [(0, 2), (0, 10), (0, 3)],
    [(0, 9), (0, 4), (0, 8)],
    [(0, 5), (0, 7), (0, 6)],
    [(9, 11), (9, 1), (9, 0)],
    [(9, 3), (9, 10), (9, 2)],
    [(9, 8), (9, 4), (9, 9)],
    [(9, 6), (9, 7), (9, 5)],
];

fn coord_flip(x: u8, y: u8) -> NocAddress {
    NocAddress {
        n0: (x, y),
        n1: (GRID_SIZE_X - x - 1, GRID_SIZE_Y - y - 1),
    }
}

pub struct Endpoints {
    pub pcie: Tile,
    pub arc: Tile,

    pub tensix: [Tile; 140],
    pub tensix_active_count: usize,
    pub tensix_broadcast: [((u8, u8), (u8, u8)); 2],
    pub use_translated_multicast: bool,

    pub dram: [[Tile; 3]; 8],
    pub dram_active_count: usize,

    pub ethernet: [Tile; 14],
    pub ethernet_active_count: usize,

    pub l2cpu: [Tile; 4],
    pub l2cpu_active_count: usize,

    pub tensix_l1_size: u64,
    pub dram_size: u64,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            pcie: Tile::default(),
            arc: Tile {
                addr: NocAddress {
                    n0: (8, 0),
                    n1: (8, 11),
                },
                align_read: 16,
                align_write: 16,
            },
            tensix: [Tile::default(); 140],
            tensix_active_count: 0,
            tensix_broadcast: [((0, 0), (0, 0)); 2],
            use_translated_multicast: false,
            dram: [[Tile::default(); 3]; 8],
            dram_active_count: 0,
            ethernet: [Tile::default(); 14],
            ethernet_active_count: 0,
            l2cpu: [Tile::default(); 4],
            l2cpu_active_count: 0,

            tensix_l1_size: 1536 * 1024,
            dram_size: 4 * 1024 * 1024 * 1024,
        }
    }
}

impl Endpoints {
    pub fn new(device: &mut Blackhole) -> Result<Endpoints, PciError> {
        let telemetry = device.get_telemetry_unchanged()?;

        let mut all_tensix = [(0, 0); 140];
        let mut index = 0;
        for y in 2..=11 {
            for x in 1..=7 {
                all_tensix[index] = (x, y);
                index += 1;
            }

            for x in 10..=16 {
                all_tensix[index] = (x, y);
                index += 1;
            }
        }

        let mut endpoints = Endpoints::default();

        if telemetry.translation_enabled() {
            let working_cols = telemetry.enabled_tensix_columns();
            for core in all_tensix {
                let x = core.0 as u32;
                if (x <= 7 && x < working_cols) || (x >= 10 && (x - 2) < working_cols) {
                    endpoints.tensix[endpoints.tensix_active_count] = Tile {
                        addr: NocAddress {
                            n0: (core.0, core.1),
                            n1: (core.0, core.1),
                        },
                        align_read: 16,
                        align_write: 16,
                    };
                    endpoints.tensix_active_count += 1;
                }
            }

            let translated_dram_coords = [
                [(17, 12), (17, 13), (17, 14)],
                [(18, 12), (18, 13), (18, 14)],
                [(17, 15), (17, 16), (17, 17)],
                [(18, 15), (18, 16), (18, 17)],
                [(17, 18), (17, 19), (17, 20)],
                [(18, 18), (18, 19), (18, 20)],
                [(17, 21), (17, 22), (17, 23)],
                [(18, 21), (18, 22), (18, 23)],
            ];

            let working_dram = telemetry.enabled_gddr();
            for _ in 0..working_dram.count_ones() {
                endpoints.dram[endpoints.dram_active_count][0] = Tile {
                    addr: NocAddress {
                        n0: translated_dram_coords[endpoints.dram_active_count][0],
                        n1: translated_dram_coords[endpoints.dram_active_count][0],
                    },
                    align_read: 64,
                    align_write: 16,
                };
                endpoints.dram[endpoints.dram_active_count][1] = Tile {
                    addr: NocAddress {
                        n0: translated_dram_coords[endpoints.dram_active_count][1],
                        n1: translated_dram_coords[endpoints.dram_active_count][1],
                    },
                    align_read: 64,
                    align_write: 16,
                };
                endpoints.dram[endpoints.dram_active_count][2] = Tile {
                    addr: NocAddress {
                        n0: translated_dram_coords[endpoints.dram_active_count][2],
                        n1: translated_dram_coords[endpoints.dram_active_count][2],
                    },
                    align_read: 64,
                    align_write: 16,
                };
                endpoints.dram_active_count += 1;
            }

            endpoints.use_translated_multicast = true;
            endpoints.tensix_broadcast = [((2, 3), (1, 2)), ((1, 2), (2, 3))];

            endpoints.pcie = Tile {
                addr: NocAddress {
                    n0: (19, 24),
                    n1: (19, 24),
                },
                align_read: 64,
                align_write: 16,
            };
        } else {
            let mut working_col_bitmask = telemetry.enabled_tensix_columns();

            let mut working_cols = Vec::new();
            let mut col = 0;
            let tensix_cols = [1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15, 16];
            while working_col_bitmask != 0 {
                if working_col_bitmask & 0x1 != 0 {
                    working_cols.push(tensix_cols[col]);
                }
                working_col_bitmask >>= 1;
                col += 1;
            }

            for core in all_tensix[..index].iter() {
                if working_cols.contains(&core.0) {
                    endpoints.tensix[endpoints.tensix_active_count] = Tile {
                        addr: coord_flip(core.0, core.1),
                        align_read: 16,
                        align_write: 16,
                    };
                    endpoints.tensix_active_count += 1;
                }
            }

            let mut working_dram = telemetry.enabled_gddr();
            let mut dram_index = 0;
            while working_dram != 0 {
                if working_dram & 0x1 != 0 {
                    endpoints.dram[endpoints.dram_active_count][0] = Tile {
                        addr: coord_flip(
                            GDDR_NOC0_COORDS[dram_index][0].0,
                            GDDR_NOC0_COORDS[dram_index][0].1,
                        ),
                        align_read: 64,
                        align_write: 16,
                    };
                    endpoints.dram[endpoints.dram_active_count][1] = Tile {
                        addr: coord_flip(
                            GDDR_NOC0_COORDS[dram_index][1].0,
                            GDDR_NOC0_COORDS[dram_index][1].1,
                        ),
                        align_read: 64,
                        align_write: 16,
                    };
                    endpoints.dram[endpoints.dram_active_count][2] = Tile {
                        addr: coord_flip(
                            GDDR_NOC0_COORDS[dram_index][2].0,
                            GDDR_NOC0_COORDS[dram_index][2].1,
                        ),
                        align_read: 64,
                        align_write: 16,
                    };
                    endpoints.dram_active_count += 1;
                }

                working_dram >>= 1;
                dram_index += 1;
            }

            endpoints.use_translated_multicast = false;

            let bcast_start = coord_flip(1, 2);
            let bcast_end = coord_flip(16, 11);
            endpoints.tensix_broadcast = [
                (bcast_start.n0, bcast_end.n0),
                (bcast_start.n1, bcast_end.n1),
            ];
        }

        Ok(endpoints)
    }
}
