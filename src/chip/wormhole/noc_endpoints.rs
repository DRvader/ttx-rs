use crate::chip::noc::{NocAddress, Tile};

const DRAM_LOCATIONS: &[[(u8, u8); 3]] = &[
    [(0, 0), (0, 1), (0, 11)],
    [(0, 5), (0, 6), (0, 7)],
    [(5, 0), (5, 1), (5, 11)],
    [(5, 2), (5, 9), (5, 10)],
    [(5, 3), (5, 4), (5, 8)],
    [(5, 5), (5, 6), (5, 7)],
];
const ETH_LOCATIONS: &[(u8, u8)] = &[
    (9, 0),
    (1, 0),
    (8, 0),
    (2, 0),
    (7, 0),
    (3, 0),
    (6, 0),
    (4, 0),
    (9, 6),
    (1, 6),
    (8, 6),
    (2, 6),
    (7, 6),
    (3, 6),
    (6, 6),
    (4, 6),
];
const ARC_LOCATION: (u8, u8) = (0, 10);
const PCI_LOCATION: (u8, u8) = (0, 3);

const GRID_SIZE_X: u8 = 10;
const GRID_SIZE_Y: u8 = 12;
const _NUM_TENSIX_X: u8 = GRID_SIZE_X - 2;
const NUM_TENSIX_Y: u8 = GRID_SIZE_Y - 2;

const _PHYS_X_TO_NOC_0_X: &[u8] = &[0, 9, 1, 8, 2, 7, 3, 6, 4, 5];
const PHYS_Y_TO_NOC_0_Y: &[u8] = &[0, 11, 1, 10, 2, 9, 3, 8, 4, 7, 5, 6];
const _PHYS_X_TO_NOC_1_X: &[u8] = &[9, 0, 8, 1, 7, 2, 6, 3, 5, 4];
const _PHYS_Y_TO_NOC_1_Y: &[u8] = &[11, 0, 10, 1, 9, 2, 8, 3, 7, 4, 6, 5];

const _ALL_TENSIX_ROWS: &[u8] = &[1, 2, 3, 4, 5, 7, 8, 9, 10, 11];
const ALL_TENSIX_COLS: &[u8] = &[1, 2, 3, 4, 6, 7, 8, 9];

fn coord_flip(x: u8, y: u8) -> NocAddress {
    NocAddress {
        n0: (x, y),
        n1: (GRID_SIZE_X - x - 1, GRID_SIZE_Y - y - 1),
    }
}

#[derive(Debug)]
pub struct NocGrid {
    pub tensix: Vec<Tile>,
    pub dram: Vec<[Tile; 3]>,
    pub pci: Tile,
    pub arc: Tile,
    pub eth: Vec<Tile>,

    pub tensix_l1_size: u64,
    pub dram_size: u64,
}

pub fn get_grid(harvest: u32) -> NocGrid {
    let mut good_cores = Vec::new();
    for y in 0..NUM_TENSIX_Y {
        let bad_row = harvest & (1 << y) != 0;

        if !bad_row {
            let y = PHYS_Y_TO_NOC_0_Y[y as usize + 1];
            for x in ALL_TENSIX_COLS.iter().copied() {
                good_cores.push(coord_flip(x, y));
            }
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
        dram: Vec::from_iter(DRAM_LOCATIONS.iter().cloned().map(|cores| {
            cores.map(|(x, y)| Tile {
                addr: coord_flip(x, y),
                align_read: 32,
                align_write: 16,
            })
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
        eth: Vec::from_iter(ETH_LOCATIONS.iter().cloned().map(|(x, y)| Tile {
            addr: coord_flip(x, y),
            align_read: 16,
            align_write: 16,
        })),
        // 1.5 MB per tensix
        tensix_l1_size: 1536 * 1024,
        // 2 GB per core
        dram_size: 2 * 1024 * 1024 * 1024,
    }
}
