use std::collections::HashSet;

use crate::chip::noc::{NocAddress, Tile};

const DRAM_LOCATIONS: &[(u8, u8)] = &[
    (1, 6),
    (4, 6),
    (7, 6),
    (10, 6),
    (1, 0),
    (4, 0),
    (7, 0),
    (10, 0),
];
const ARC_LOCATION: (u8, u8) = (0, 2);
const PCI_LOCATION: (u8, u8) = (0, 4);
const GRID_SIZE_X: u8 = 13;
const GRID_SIZE_Y: u8 = 12;
const _NUM_TENSIX_X: u8 = GRID_SIZE_X - 1;
const _NUM_TENSIX_Y: u8 = GRID_SIZE_Y - 2;

const _PHYS_X_TO_NOC_0_X: &[u8] = &[0, 12, 1, 11, 2, 10, 3, 9, 4, 8, 5, 7, 6];
const PHYS_Y_TO_NOC_0_Y: &[u8] = &[0, 11, 1, 10, 2, 9, 3, 8, 4, 7, 5, 6];
const _PHYS_X_TO_NOC_1_X: &[u8] = &[12, 0, 11, 1, 10, 2, 9, 3, 8, 4, 7, 5, 6];
const _PHYS_Y_TO_NOC_1_Y: &[u8] = &[11, 0, 10, 1, 9, 2, 8, 3, 7, 4, 6, 5];

#[derive(Debug)]
pub struct NocGrid {
    pub tensix: Vec<Tile>,
    pub dram: Vec<Tile>,
    pub pci: Tile,
    pub arc: Tile,

    pub tensix_l1_size: u64,
    pub dram_size: u64,
}

fn coord_flip(x: u8, y: u8) -> NocAddress {
    NocAddress {
        n0: (x, y),
        n1: (GRID_SIZE_X - x - 1, GRID_SIZE_Y - y - 1),
    }
}

pub fn get_grid(harvest: u32) -> NocGrid {
    let mut bad_rows = harvest << 1;
    let mut disabled_rows = HashSet::new();
    for y in 0..32 {
        if bad_rows & 1 == 1 {
            disabled_rows.insert(PHYS_Y_TO_NOC_0_Y[(GRID_SIZE_Y - y - 1) as usize]);
        }
        bad_rows >>= 1;
    }

    let good_rows = HashSet::from_iter([1, 2, 3, 4, 5, 7, 8, 9, 10, 11])
        .difference(&disabled_rows)
        .cloned()
        .collect::<Vec<_>>();

    let mut good_cores = Vec::new();
    for y in good_rows {
        for x in 1..GRID_SIZE_X {
            good_cores.push(coord_flip(x, y));
        }
    }

    NocGrid {
        tensix: good_cores
            .into_iter()
            .map(|addr| Tile {
                addr,
                align_read: 16,
                align_write: 16,
            })
            .collect(),
        dram: Vec::from_iter(DRAM_LOCATIONS.iter().cloned().map(|(x, y)| Tile {
            addr: coord_flip(x, y),
            align_read: 32,
            align_write: 16,
        })),
        pci: Tile {
            addr: coord_flip(PCI_LOCATION.0, PCI_LOCATION.1),
            align_read: 32,
            align_write: 16,
        },
        arc: Tile {
            addr: coord_flip(ARC_LOCATION.0, ARC_LOCATION.1),
            align_read: 16,
            align_write: 16,
        },

        // 1MB per tensix
        tensix_l1_size: 1024 * 1024,
        // 1GB per core
        dram_size: 1024 * 1024 * 1024,
    }
}
