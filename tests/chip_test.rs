use ttkmd_if::PciDevice;

use ttx_rs::chip::{
    noc::{NocId, NocInterface},
    {self, Chip},
};

#[test]
fn read_write_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let addr = 3;
        let aligned_addr = (addr + 3) & !3;

        let noc_id = NocId::Noc0;
        let tile = match &chip {
            Chip::Grayskull(grayskull) => grayskull.endpoints.tensix[0],
            Chip::Wormhole(wormhole) => wormhole.endpoints.tensix[0],
            Chip::Blackhole(blackhole) => blackhole.endpoints.tensix[0],
        };

        chip.noc_write32(noc_id, tile, aligned_addr, 0xfaca);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr);
        assert_eq!(readback, 0xfaca, "{readback:x} != faca");

        chip.noc_write32(noc_id, tile, aligned_addr, 0xcdcd_cdcd);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr);
        assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

        chip.noc_write32(noc_id, tile, aligned_addr + 4, 0xcdcd_cdcd);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 4);
        assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

        chip.noc_write32(noc_id, tile, aligned_addr + 1, 0xdead);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr);
        assert_eq!(readback, 0xdeadcd, "{readback:x} != deadcd");
        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 4);
        assert_eq!(readback, 0xcdcdcd00, "{readback:x} != 00cdcdcd");

        chip.noc_write32(noc_id, tile, aligned_addr, 0xcdcd_cdcd);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr);
        assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

        chip.noc_write32(noc_id, tile, aligned_addr + 4, 0xcdcd_cdcd);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 4);
        assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

        chip.noc_write32(noc_id, tile, aligned_addr + 3, 0xc0ffe);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr);
        assert_eq!(readback, 0xfecdcdcd, "{readback:x} != fecdcdcd");
        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 4);
        assert_eq!(readback, 0xcd000c0f, "{readback:x} != c0f");

        chip.noc_write32(noc_id, tile, aligned_addr, 0x01234567);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr);
        assert_eq!(readback, 0x01234567, "{readback:x} != 01234567");

        chip.noc_write32(noc_id, tile, aligned_addr + 4, 0xabcdef);
        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 4);
        assert_eq!(readback, 0xabcdef, "{readback:x} != abcdef");

        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 1);
        assert_eq!(readback, 0xef012345, "{readback:x} != ef012345");

        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 3);
        assert_eq!(readback, 0xabcdef01, "{readback:x} != abcdef01");

        // Block write test
        let mut write_buffer = Vec::new();
        write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
        write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
        chip.noc_write(noc_id, tile, aligned_addr, &write_buffer);

        let mut readback_buffer = vec![0u8; write_buffer.len()];
        chip.noc_read(noc_id, tile, aligned_addr, &mut readback_buffer);
        assert_eq!(write_buffer, readback_buffer);

        let write_buffer = [0xad, 0xde];
        chip.noc_write(noc_id, tile, aligned_addr + 1, &write_buffer);

        let mut readback_buffer = vec![0u8; 4];
        chip.noc_read(noc_id, tile, aligned_addr, &mut readback_buffer);
        assert_eq!([0xcd, 0xad, 0xde, 0xcd], readback_buffer.as_slice());

        let mut write_buffer = Vec::new();
        write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
        write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
        chip.noc_write(noc_id, tile, aligned_addr, &write_buffer);

        let mut readback_buffer = vec![0u8; write_buffer.len()];
        chip.noc_read(noc_id, tile, aligned_addr, &mut readback_buffer);
        assert_eq!(write_buffer, readback_buffer);

        let write_buffer = [0xad, 0xde];
        chip.noc_write(noc_id, tile, aligned_addr + 3, &write_buffer);

        let mut readback_buffer = vec![0u8; 7];
        chip.noc_read(noc_id, tile, aligned_addr, &mut readback_buffer);
        assert_eq!(
            [0xcd, 0xcd, 0xcd, 0xad, 0xde, 0xcd, 0xcd],
            readback_buffer.as_slice()
        );

        let mut write_buffer = Vec::new();
        write_buffer.extend(0x01234567u32.to_le_bytes());
        write_buffer.extend(0xabcdefu32.to_le_bytes());
        chip.noc_write(noc_id, tile, aligned_addr, &write_buffer);

        let mut readback_buffer = vec![0u8; write_buffer.len()];
        chip.noc_read(noc_id, tile, aligned_addr, &mut readback_buffer);
        assert_eq!(write_buffer, readback_buffer);

        let readback = chip.noc_read32(noc_id, tile, aligned_addr + 1);
        assert_eq!(readback, 0xef012345, "{readback:x} != ef012345");

        let mut readback_buffer = vec![0u8; 4];
        chip.noc_read(noc_id, tile, aligned_addr + 1, &mut readback_buffer);
        assert_eq!([0x45, 0x23, 0x01, 0xef], readback_buffer.as_slice());

        let mut readback_buffer = vec![0u8; 4];
        chip.noc_read(noc_id, tile, aligned_addr + 3, &mut readback_buffer);
        assert_eq!([0x01, 0xef, 0xcd, 0xab], readback_buffer.as_slice());

        let mut write_buffer = vec![0; 1024];
        for (index, r) in write_buffer.iter_mut().enumerate() {
            *r = index as u8;
        }
        chip.noc_write(noc_id, tile, aligned_addr, &write_buffer);

        let mut readback_buffer = vec![0u8; write_buffer.len()];
        chip.noc_read(noc_id, tile, aligned_addr, &mut readback_buffer);
        assert_eq!(write_buffer, readback_buffer);

        let mut write_buffer = vec![0; 1024];
        for (index, r) in write_buffer.iter_mut().enumerate() {
            *r = index as u8;
        }
        chip.noc_write(noc_id, tile, aligned_addr, &write_buffer);

        let mut readback_buffer = vec![0u8; write_buffer.len()];
        chip.noc_read(noc_id, tile, aligned_addr + 3, &mut readback_buffer);
        assert_eq!(
            write_buffer[3..],
            readback_buffer[..readback_buffer.len() - 3]
        );

        let mut write_buffer = vec![0; 1024];
        for (index, r) in write_buffer.iter_mut().enumerate() {
            *r = index as u8;
        }
        chip.noc_write(noc_id, tile, aligned_addr + 1, &write_buffer);

        let mut readback_buffer = vec![0u8; write_buffer.len()];
        chip.noc_read(noc_id, tile, aligned_addr + 1, &mut readback_buffer);
        assert_eq!(write_buffer, readback_buffer);
    }
}

#[test]
fn broadcast_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        chip.noc_broadcast32(NocId::Noc0, 0, 0xfaca);
        for tensix in 0..chip.tensix_count() {
            let tile = chip.tensix(tensix);
            let readback = chip.noc_read32(NocId::Noc0, tile, 0);
            assert_eq!(
                readback, 0xfaca,
                "Failed to broadcast to {tile:?} on Noc0 while testing {chip}"
            );
        }

        chip.noc_broadcast32(NocId::Noc1, 0x100, 0xfada);
        for tensix in 0..chip.tensix_count() {
            let tile = chip.tensix(tensix);
            let readback = chip.noc_read32(NocId::Noc0, tile, 0x100);
            assert_eq!(
                readback, 0xfada,
                "Failed to broadcast to {tile:?} on Noc1 while testing {chip}"
            );
        }
    }
}

#[test]
fn arc_postcode_sanity() {
    for id in PciDevice::scan() {
        let chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        if chip.arch().is_wormhole() || chip.arch().is_grayskull() {
            let postcode = chip.device().read32(0x1ff30060).unwrap();
            assert_eq!(postcode >> 16, 0xC0DE);
        }
    }
}

#[test]
fn arc_read_write_test() {
    for id in PciDevice::scan() {
        let chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        if let Chip::Wormhole(mut wormhole) = chip {
            let dump_addr = wormhole
                .send_arc_msg(chip::wormhole::ArcMsg::GetSpiDumpAddr)
                .unwrap()
                .arg();

            assert!(dump_addr > 0);

            let raw_device = &mut wormhole.interface.device;

            let csm_offset = 0x1fe80000 - 0x10000000_u64;

            let addr = csm_offset + (dump_addr as u64);

            let aligned_addr = (addr + 3) & !3;

            raw_device.write32(aligned_addr as u32, 0xfaca).unwrap();
            let readback = raw_device.read32(aligned_addr as u32).unwrap();
            assert_eq!(readback, 0xfaca, "{readback:x} != faca");

            raw_device
                .write32(aligned_addr as u32, 0xcdcd_cdcd)
                .unwrap();
            let readback = raw_device.read32(aligned_addr as u32).unwrap();
            assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

            raw_device
                .write32(aligned_addr as u32 + 4, 0xcdcd_cdcd)
                .unwrap();
            let readback = raw_device.read32(aligned_addr as u32 + 4).unwrap();
            assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

            raw_device.write32(aligned_addr as u32 + 1, 0xdead).unwrap();
            let readback = raw_device.read32(aligned_addr as u32).unwrap();
            assert_eq!(readback, 0xdeadcd, "{readback:x} != deadcd");
            let readback = raw_device.read32(aligned_addr as u32 + 4).unwrap();
            assert_eq!(readback, 0xcdcdcd00, "{readback:x} != 00cdcdcd");

            raw_device
                .write32(aligned_addr as u32, 0xcdcd_cdcd)
                .unwrap();
            let readback = raw_device.read32(aligned_addr as u32).unwrap();
            assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

            raw_device
                .write32(aligned_addr as u32 + 4, 0xcdcd_cdcd)
                .unwrap();
            let readback = raw_device.read32(aligned_addr as u32 + 4).unwrap();
            assert_eq!(readback, 0xcdcd_cdcd, "{readback:x} != cdcdcdcd");

            raw_device
                .write32(aligned_addr as u32 + 3, 0xc0ffe)
                .unwrap();
            let readback = raw_device.read32(aligned_addr as u32).unwrap();
            assert_eq!(readback, 0xfecdcdcd, "{readback:x} != fecdcdcd");
            let readback = raw_device.read32(aligned_addr as u32 + 4).unwrap();
            assert_eq!(readback, 0xcd000c0f, "{readback:x} != c0f");

            raw_device.write32(aligned_addr as u32, 0x01234567).unwrap();
            let readback = raw_device.read32(aligned_addr as u32).unwrap();
            assert_eq!(readback, 0x01234567, "{readback:x} != 01234567");

            raw_device
                .write32(aligned_addr as u32 + 4, 0xabcdef)
                .unwrap();
            let readback = raw_device.read32(aligned_addr as u32 + 4).unwrap();
            assert_eq!(readback, 0xabcdef, "{readback:x} != abcdef");

            let readback = raw_device.read32(aligned_addr as u32 + 1).unwrap();
            assert_eq!(readback, 0xef012345, "{readback:x} != ef012345");

            let readback = raw_device.read32(aligned_addr as u32 + 3).unwrap();
            assert_eq!(readback, 0xabcdef01, "{readback:x} != abcdef01");

            // Block write test
            let mut write_buffer = Vec::new();
            write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
            write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
            raw_device
                .write_block(aligned_addr as u32, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; write_buffer.len()];
            raw_device
                .read_block(aligned_addr as u32, &mut readback_buffer)
                .unwrap();
            assert_eq!(write_buffer, readback_buffer);

            let write_buffer = [0xad, 0xde];
            raw_device
                .write_block(aligned_addr as u32 + 1, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; 4];
            raw_device
                .read_block(aligned_addr as u32, &mut readback_buffer)
                .unwrap();
            assert_eq!([0xcd, 0xad, 0xde, 0xcd], readback_buffer.as_slice());

            let mut write_buffer = Vec::new();
            write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
            write_buffer.extend(0xcdcd_cdcdu32.to_le_bytes());
            raw_device
                .write_block(aligned_addr as u32, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; write_buffer.len()];
            raw_device
                .read_block(aligned_addr as u32, &mut readback_buffer)
                .unwrap();
            assert_eq!(write_buffer, readback_buffer);

            let write_buffer = [0xad, 0xde];
            raw_device
                .write_block(aligned_addr as u32 + 3, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; 7];
            raw_device
                .read_block(aligned_addr as u32, &mut readback_buffer)
                .unwrap();
            assert_eq!(
                [0xcd, 0xcd, 0xcd, 0xad, 0xde, 0xcd, 0xcd],
                readback_buffer.as_slice()
            );

            let mut write_buffer = Vec::new();
            write_buffer.extend(0x01234567u32.to_le_bytes());
            write_buffer.extend(0xabcdefu32.to_le_bytes());
            raw_device
                .write_block(aligned_addr as u32, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; write_buffer.len()];
            raw_device
                .read_block(aligned_addr as u32, &mut readback_buffer)
                .unwrap();
            assert_eq!(write_buffer, readback_buffer);

            let readback = raw_device.read32(aligned_addr as u32 + 1).unwrap();
            assert_eq!(readback, 0xef012345, "{readback:x} != ef012345");

            let mut readback_buffer = vec![0u8; 4];
            raw_device
                .read_block(aligned_addr as u32 + 1, &mut readback_buffer)
                .unwrap();
            assert_eq!([0x45, 0x23, 0x01, 0xef], readback_buffer.as_slice());

            let mut readback_buffer = vec![0u8; 4];
            raw_device
                .read_block(aligned_addr as u32 + 3, &mut readback_buffer)
                .unwrap();
            assert_eq!([0x01, 0xef, 0xcd, 0xab], readback_buffer.as_slice());

            let mut write_buffer = vec![0; 1024];
            for (index, r) in write_buffer.iter_mut().enumerate() {
                *r = index as u8;
            }
            raw_device
                .write_block(aligned_addr as u32, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; write_buffer.len()];
            raw_device
                .read_block(aligned_addr as u32, &mut readback_buffer)
                .unwrap();
            assert_eq!(write_buffer, readback_buffer);

            let mut write_buffer = vec![0; 1024];
            for (index, r) in write_buffer.iter_mut().enumerate() {
                *r = index as u8;
            }
            raw_device
                .write_block(aligned_addr as u32, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; write_buffer.len()];
            raw_device
                .read_block(aligned_addr as u32 + 3, &mut readback_buffer)
                .unwrap();
            assert_eq!(
                write_buffer[3..],
                readback_buffer[..readback_buffer.len() - 3]
            );

            let mut write_buffer = vec![0; 1024];
            for (index, r) in write_buffer.iter_mut().enumerate() {
                *r = index as u8;
            }
            raw_device
                .write_block(aligned_addr as u32 + 1, &write_buffer)
                .unwrap();

            let mut readback_buffer = vec![0u8; write_buffer.len()];
            raw_device
                .read_block(aligned_addr as u32 + 1, &mut readback_buffer)
                .unwrap();
            assert_eq!(write_buffer, readback_buffer);
        }
    }
}

#[test]
fn arc_msg_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let input = 100;

        let (rc, result) = match &mut chip {
            Chip::Grayskull(grayskull) => {
                let result = grayskull
                    .send_arc_msg(chip::grayskull::ArcMsg::Test { arg: input })
                    .unwrap();

                (result.rc(), result.arg())
            }
            Chip::Wormhole(wormhole) => {
                let result = wormhole
                    .send_arc_msg(chip::wormhole::ArcMsg::Test { arg: input })
                    .unwrap();

                (result.rc(), result.arg())
            }
            Chip::Blackhole(_blackhole) => (0, input + 1),
        };

        assert_eq!(rc, 0, "For {}[{id}] ARC msg failed", chip.arch());
        assert_eq!(
            result,
            input + 1,
            "For {}[{id}] ARC test msg failed",
            chip.arch()
        );
    }
}
