use std::path::PathBuf;

use luwen::ttkmd_if::PciDevice;
use tempfile::TempDir;

use ttx_rs::{
    chip::{
        self,
        noc::{NocId, NocInterface, Tile},
        Chip,
    },
    kernel::{Kernel, KernelData},
};

#[ctor::ctor]
fn test_init() {
    tracing_subscriber::util::SubscriberInitExt::init(
        tracing_subscriber::layer::SubscriberExt::with(
            tracing_subscriber::layer::SubscriberExt::with(
                tracing_subscriber::registry(),
                tracing_subscriber::fmt::layer(),
            ),
            tracing_subscriber::filter::EnvFilter::from_default_env(),
        ),
    );
}

fn write_cargo_toml(dir: &TempDir) {
    std::fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            r#"
    [package]
    name = "kernel"
    version = "0.1.0"
    edition = "2024"

    [dependencies]
    tensix-std = {{path = "{}/../tensix-std"}}
    "#,
            env!("CARGO_MANIFEST_DIR"),
        ),
    )
    .unwrap();
}

fn write_main(src_file: &PathBuf, test: &str) {
    std::fs::write(
        src_file,
        format!(
            r#"
    #![no_std]
    #![no_main]

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {{
        unsafe {{
            let mut i = 0;
            while i < n {{
                dest.add(i).write(src.add(i).read());
                i += 1;
            }}
            dest
        }}
    }}

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memset(dest: *mut u8, src: core::ffi::c_int, n: usize) -> *mut u8 {{
        unsafe {{
            let mut i = 0;
            while i < n {{
                dest.add(i).write(src as u8);
                i += 1;
            }}
            dest
        }}
    }}

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memcmp(lhs: *const core::ffi::c_void, rhs: *const core::ffi::c_void, count: usize) -> core::ffi::c_int {{
        unsafe {{
            for i in 0..count {{
                let a = lhs.cast::<u8>().offset(i as isize).read();
                let b = rhs.cast::<u8>().offset(i as isize).read();

                let cmp = a as i32 - b as i32;
                if cmp != 0 {{
                    return cmp;
                }}
            }}
        }}

        return 0;
    }}

    #[repr(align(64))]
    struct NocAligned<T>(T);

    impl<T> core::ops::Deref for NocAligned<T> {{
        type Target = T;

        fn deref(&self) -> &<Self as core::ops::Deref>::Target {{
            &self.0
        }}
    }}

    impl<T> core::ops::DerefMut for NocAligned<T> {{
        fn deref_mut(&mut self) -> &mut <Self as core::ops::Deref>::Target {{
            &mut self.0
        }}
    }}

    #[repr(transparent)]
    struct SyncUnsafeCell<T>(core::cell::UnsafeCell<T>);
    unsafe impl<T: Sync> Sync for SyncUnsafeCell<T> {{}}

    #[repr(transparent)]
    pub struct SyncUnsafeNocCell<T>(NocAligned<SyncUnsafeCell<T>>);

    impl<T> SyncUnsafeNocCell<T> {{
        pub const fn new(value: T) -> Self {{
            SyncUnsafeNocCell(NocAligned(SyncUnsafeCell(core::cell::UnsafeCell::new(value))))
        }}

        pub fn get(&self) -> *mut T {{
            ((self.0).0).0.get()
        }}
    }}

    type SYNC<T> = SyncUnsafeNocCell<T>;

    #[unsafe(no_mangle)]
    pub static CORE_ID: SYNC<i32> = SYNC::new(-1);

    struct Tile {{
        n0: (u8, u8),
        n1: (u8, u8)
    }}

    impl From<u32> for Tile {{
        fn from(value: u32) -> Self {{
            Tile {{
                n0: (value as u8, (value >> 8) as u8),
                n1: ((value >> 16) as u8, (value >> 24) as u8)
            }}
        }}
    }}

    {test}
    "#
        ),
    )
    .unwrap()
}

fn build_test(chip: &mut Chip, noc_id: NocId, tile: Tile, file: &str, wait: bool) -> Kernel {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();

    let src_file = dir.path().join("src").join("main.rs");

    write_cargo_toml(&dir);
    write_main(&src_file, file);

    let kernel_data = chip::loader::build_kernel(
        "test",
        chip.arch(),
        chip::loader::LoadOptions::new(dir.path()).hide_output(),
        None,
    );

    chip.load_kernel(kernel_data, noc_id, tile, wait)
}

#[allow(unused)]
fn build_tests(chip: &mut Chip, tiles: Option<Vec<Tile>>, file: &str, wait: bool) -> KernelData {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();

    let src_file = dir.path().join("src").join("main.rs");

    write_cargo_toml(&dir);
    write_main(&src_file, file);

    let mut kernel_data = chip::loader::build_kernel(
        "test",
        chip.arch(),
        chip::loader::LoadOptions::new(dir.path()).hide_output(),
        None,
    );

    chip.load_kernels(&mut kernel_data, tiles, wait);

    kernel_data
}

macro_rules! rust_test {
    ($chip:ident, $noc_id:expr, $tile:expr, {$($t:tt)*}) => {{
        let __tile = $tile;
        build_test(
            &mut $chip,
            $noc_id,
            __tile,
            core::stringify!($($t)*),
            true
        )
    }};

    (nowait, $chip:ident, $noc_id:expr, $tile:expr, {$($t:tt)*}) => {{
        let __tile = $tile;
        build_test(
            &mut $chip,
            $noc_id,
            __tile,
            core::stringify!($($t)*),
            false
        )
    }};
}

#[test]
fn hello_world() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        rust_test! {
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::entry;

                #[entry(brisc)]
                unsafe fn entry() {

                }
            }
        };
    }
}

#[test]
fn panic() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let mut kernel = rust_test! {
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::entry;

                #[entry(brisc)]
                unsafe fn entry() {
                    panic!("PANIC'D");
                }
            }
        };

        assert!(kernel.check_panic());
    }
}

#[test]
fn noc_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let mut kernel_a = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(1),
            {
                use tensix_std::entry;

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 128]> = NocAligned([0; 128]);

                #[entry(brisc)]
                unsafe fn entry() {
                    unsafe {
                        unsafe fn set_pc(pc: u16) {
                            unsafe {
                                tensix_std::set_postcode_brisc(0xc0de0000 | pc as u32);
                            }
                        }

                        let buf = &raw mut NOC_BUFFER.0[0];

                        // Find index for host to write data into
                        let mut index = 16;
                        while buf.add(index) as usize % 16 != 0 {
                            index += 1;
                        }
                        buf.add(6).write_volatile(buf.add(index) as u32);

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        buf.add(10).write_volatile(0xfaca);

                        buf.add(2).write_volatile(buf.add(1).read_volatile() + 1);

                        buf.add(3)
                            .write_volatile((0xffbec as *mut u32).read_volatile() + 1);

                        set_pc(0x199);

                        let data = core::slice::from_raw_parts(buf.add(index).cast::<u8>(), 4);
                        let noc_coords = buf.add(5).read_volatile();
                        set_pc(0x198);

                        let reconstructed_data = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                        buf.add(11).write_volatile(reconstructed_data);

                        set_pc(0x199);

                        tensix_std::target::noc::noc_write(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: buf.add(4).read_volatile() as u64,
                                x_end: noc_coords as u8,
                                y_end: (noc_coords >> 8) as u8,
                                ..Default::default()
                            },
                            data,
                            true,
                        );

                        set_pc(0x200);

                        // Now test read
                        buf.add(index).write_volatile(0);
                        let data = core::slice::from_raw_parts_mut(buf.add(index).cast::<u8>(), 4);
                        tensix_std::target::noc::noc_read(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: buf.add(4).read_volatile() as u64 + 16,
                                x_end: noc_coords as u8,
                                y_end: (noc_coords >> 8) as u8,
                                ..Default::default()
                            },
                            data,
                            true,
                        );
                        let data = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

                        set_pc(data as u16);

                        while data != 0xbad {}

                        set_pc(0x202);
                    }
                }
            }
        };

        let mut kernel_b = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::entry;

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 128]> = NocAligned([0; 128]);

                #[entry(brisc)]
                unsafe fn entry() {
                    unsafe {
                        let buf = &raw mut NOC_BUFFER.0[0];

                        let mut index = 5;
                        while buf.add(index) as usize % 16 != 0 {
                            index += 1;
                        }
                        buf.add(index).write_volatile(0);
                        buf.add(1).write_volatile(buf.add(index) as u32);

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        while buf.add(index).read_volatile() != 0xdaca {}
                    }
                }
            }
        };

        let buffer_a = kernel_a["NOC_BUFFER"];
        let buffer_b = kernel_b["NOC_BUFFER"];

        let kernal_a_tile = chip.tensix(1);
        let kernal_b_tile = chip.tensix(0);

        let mut _chip = chip.dupe().unwrap();
        let mut write_a = |addr, value| {
            _chip.noc_write32(NocId::Noc0, kernal_a_tile, addr, value);
        };

        let mut _chip = chip.dupe().unwrap();
        let mut read_a = |addr| _chip.noc_read32(NocId::Noc0, kernal_a_tile, addr);
        let mut _chip = chip.dupe().unwrap();
        let mut write_buffer_a = |offset: u64, value| {
            _chip.noc_write32(NocId::Noc0, kernal_a_tile, buffer_a + (4 * offset), value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut read_buffer_a =
            |offset: u64| _chip.noc_read32(NocId::Noc0, kernal_a_tile, buffer_a + (4 * offset));
        let mut _chip = chip.dupe().unwrap();
        let mut read_b = |addr| _chip.noc_read32(NocId::Noc0, kernal_b_tile, addr);
        let mut _chip = chip.dupe().unwrap();
        let mut write_b = |addr, value| {
            _chip.noc_write32(NocId::Noc0, kernal_b_tile, addr, value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut write_buffer_b = |offset: u64, value| {
            _chip.noc_write32(NocId::Noc0, kernal_b_tile, buffer_b + (4 * offset), value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut read_buffer_b =
            |offset: u64| _chip.noc_read32(NocId::Noc0, kernal_b_tile, buffer_b + (4 * offset));

        println!("\tWaiting for sync");

        // Wait for sync
        while read_buffer_a(0) != 1 {}
        println!("\tA SYNCED");
        while read_buffer_b(0) != 1 {}
        println!("\tB SYNCED");

        println!("\tSENDING DATA FROM HOST");

        write_buffer_a(1, 0xfaca);

        // Get address A will write to b from (we want to put our data here).
        let a_to_b_src_addr = read_buffer_a(6) as u64;

        // Write the data to send to b
        write_a(a_to_b_src_addr, 0xdaca);

        // Get address for A to send the data to
        let a_to_b_dst_addr = read_buffer_b(1);

        // Send this address to A to use in the noc write
        write_buffer_a(4, a_to_b_dst_addr);
        // Alongside the coords of b
        write_buffer_a(5, kernal_b_tile.into());

        write_b(a_to_b_dst_addr as u64 + 16, 0xbad);

        println!("\tSTART A");
        write_buffer_a(0, 2);

        println!("\tSTART B");
        write_buffer_b(0, 2);

        while read_buffer_a(0) != 3 {}
        println!("\tSYNC'd A");
        while read_buffer_b(0) != 3 {}
        println!("\tSYNC'd B");

        let data = read_buffer_a(1);

        println!("\tValue to write from a -> b: {:04x}", data);

        let data = read_buffer_a(2);

        println!("\tValue to write from a -> b + 1 {:04x}", data);

        let data = read_buffer_a(3);

        println!("\tdebug {:04x}", data);

        let data = [0u32; 4];
        chip.noc_read(NocId::Noc0, kernal_a_tile, buffer_a, unsafe {
            core::slice::from_raw_parts_mut(
                data.as_ptr() as *mut u8,
                data.len() * core::mem::size_of::<u32>(),
            )
        });

        println!("\t{:04x}", data[1]);
        println!("\t{:04x}", data[2]);

        println!("\tWaiting for end");

        let data = read_buffer_a(11);
        println!("\tData to write from a -> b: {:x}", data);
        let data = read_buffer_a(5);
        println!("\tCoordinate of b: {:x}", data);

        kernel_a.print_state_diff();

        let data = read_b(a_to_b_dst_addr as u64);
        let data1 = read_b(a_to_b_dst_addr as u64 + 16);

        println!("\tData sent from a -> b: {:04x}", data);
        println!("\tData sent from b -> a: {:04x}", data1);

        kernel_b.wait_id(NocId::Noc0);
        println!("\tB COPMLETED");

        let data = read_b(a_to_b_dst_addr as u64);
        let data1 = read_a(a_to_b_src_addr);

        println!("\tData sent from a -> b: {:04x}", data);
        println!("\tData recieved from b -> a: {:04x}", data1);

        kernel_a.print_state();

        kernel_a.wait();
        println!("\tA COPMLETED");
    }
}

#[test]
fn dma_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let mut dma = chip.alloc_dma(1024);
        for i in 0..dma.buffer.len() {
            dma.buffer[i] = 0xa5;
        }

        let mut kernel = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::{entry, target::noc_map::pci_read};

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 1024]> = NocAligned([0; 1024]);

                #[entry(brisc)]
                unsafe fn brisc_main() {
                    unsafe {
                        unsafe fn set_pc(pc: u16) {
                            unsafe {
                                tensix_std::set_postcode_brisc(0xc0de0000 | pc as u32);
                            }
                        }

                        let buf = &raw mut NOC_BUFFER.0[0];

                        let mut index = 14;
                        while buf.add(index) as usize % 64 != 0 {
                            index += 1;
                        }
                        buf.add(4).write_volatile(index as u32);

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        set_pc(0x101);

                        let data = core::slice::from_raw_parts_mut(buf.add(index).cast::<u8>(), 4);
                        pci_read(
                            data,
                            ((buf.add(12).read_volatile() as u64) << 32) | (buf.add(8).read_volatile() as u64),
                        );

                        set_pc(0x100);
                    }
                }
            }
        };

        let paddr = (dma.physical_address + 63) & !63;
        let offset = paddr - dma.physical_address;

        let to_write_value = 0xfacau32;
        let value = to_write_value.to_le_bytes();

        dma.buffer[offset as usize..][0] = value[0];
        dma.buffer[offset as usize..][1] = value[1];
        dma.buffer[offset as usize..][2] = value[2];
        dma.buffer[offset as usize..][3] = value[3];

        let buffer = kernel["NOC_BUFFER"];

        println!("Waiting for start");

        let kernel_tensix = chip.tensix(0);

        let mut _chip = chip.dupe().unwrap();
        let mut write = |addr, value| {
            _chip.noc_write32(NocId::Noc0, kernel_tensix, addr, value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut read = |addr| _chip.noc_read32(NocId::Noc0, kernel_tensix, addr);

        while read(buffer) != 1 {}

        println!("Started");

        write(buffer + (4 * 8), paddr as u32);
        write(buffer + (4 * 12), (paddr >> 32) as u32);
        let index = read(buffer + (4 * 4));

        write(buffer, 2);
        while read(buffer) != 3 {}

        println!("Waiting for end");

        kernel.wait();

        println!("Ended");

        let readback_value = read(buffer + (index as u64 * 4));
        println!("READBACK[{index}]: {readback_value:x}");
        assert_eq!(
            to_write_value, readback_value,
            "{to_write_value:x} != {readback_value:x}"
        );
    }
}

#[test]
fn manual_dma_read_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let mut dma = chip.alloc_dma(1024);
        for i in 0..dma.buffer.len() {
            dma.buffer[i] = 0xa5;
        }

        let mut kernel = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::{entry, target::noc_map::pci_read};

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 1024]> = NocAligned([0; 1024]);

                #[entry(brisc)]
                unsafe fn brisc_main() {
                    unsafe {
                        unsafe fn set_pc(pc: u16) {
                            unsafe {
                                tensix_std::set_postcode_brisc(0xc0de0000 | pc as u32);
                            }
                        }

                        let buf = &raw mut NOC_BUFFER.0[0];

                        let mut index = 14;
                        while buf.add(index) as usize % 64 != 0 {
                            index += 1;
                        }
                        buf.add(4).write_volatile(index as u32);

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        set_pc(0x101);

                        let noc_coords = buf.add(10).read_volatile();
                        let data = core::slice::from_raw_parts_mut(buf.add(index).cast::<u8>(), 4);
                        data.fill(0);
                        tensix_std::target::noc::noc_read(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: ((buf.add(9).read_volatile() as u64) << 32) | (buf.add(8).read_volatile() as u64),
                                x_end: noc_coords as u8,
                                y_end: (noc_coords >> 8) as u8,
                                ..Default::default()
                            },
                            data,
                            true,
                        );

                        set_pc(0x100);
                    }
                }
            }
        };

        let paddr = (dma.physical_address + 63) & !63;
        let offset = paddr - dma.physical_address;

        let to_write_value = 0xfacau32;
        let value = to_write_value.to_le_bytes();

        dma.buffer[offset as usize..][0] = value[0];
        dma.buffer[offset as usize..][1] = value[1];
        dma.buffer[offset as usize..][2] = value[2];
        dma.buffer[offset as usize..][3] = value[3];

        let buffer = kernel["NOC_BUFFER"];

        println!("Waiting for start");

        let kernel_tensix = chip.tensix(0);

        let mut _chip = chip.dupe().unwrap();
        let mut write = |addr, value| {
            _chip.noc_write32(NocId::Noc0, kernel_tensix, addr, value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut read = |addr| _chip.noc_read32(NocId::Noc0, kernel_tensix, addr);

        while read(buffer) != 1 {}

        println!("Started");

        let dma_addr = chip.pcie_access(paddr);
        write(buffer + (4 * 8), dma_addr as u32);
        write(buffer + (4 * 9), (dma_addr >> 32) as u32);
        write(buffer + (4 * 10), chip.pcie().into());
        let index = read(buffer + (4 * 4));

        write(buffer, 2);
        while read(buffer) != 3 {}

        println!("Waiting for end");

        kernel.wait();

        println!("Ended");

        let readback_value = read(buffer + (index as u64 * 4));
        println!("READBACK[{index}]: {readback_value:x}");
        assert_eq!(
            to_write_value, readback_value,
            "{to_write_value:x} != {readback_value:x}"
        );
    }
}

#[test]
fn manual_dma_write_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let mut dma = chip.alloc_dma(1024);
        for i in 0..dma.buffer.len() {
            dma.buffer[i] = 0xa5;
        }

        let mut kernel = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::{entry, target::noc_map::pci_read};

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 1024]> = NocAligned([0; 1024]);

                #[entry(brisc)]
                unsafe fn brisc_main() {
                    unsafe {
                        unsafe fn set_pc(pc: u16) {
                            unsafe {
                                tensix_std::set_postcode_brisc(0xc0de0000 | pc as u32);
                            }
                        }

                        let buf = &raw mut NOC_BUFFER.0[0];

                        let mut index = 14;
                        while buf.add(index) as usize % 64 != 0 {
                            index += 1;
                        }
                        buf.add(4).write_volatile(index as u32);

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        set_pc(0x101);

                        buf.add(index).write_volatile(0xfaca);
                        let noc_coords = buf.add(10).read_volatile();
                        let data = core::slice::from_raw_parts_mut(buf.add(index).cast::<u8>(), 4);
                        tensix_std::target::noc::noc_write(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: ((buf.add(9).read_volatile() as u64) << 32) | (buf.add(8).read_volatile() as u64),
                                x_end: noc_coords as u8,
                                y_end: (noc_coords >> 8) as u8,
                                ..Default::default()
                            },
                            data,
                            true,
                        );

                        set_pc(0x100);
                    }
                }
            }
        };

        let paddr = (dma.physical_address + 15) & !15;
        let offset = paddr - dma.physical_address;

        let buffer = kernel["NOC_BUFFER"];

        println!("Waiting for start");

        let kernel_tensix = chip.tensix(0);

        let mut _chip = chip.dupe().unwrap();
        let mut write = |addr, value| {
            _chip.noc_write32(NocId::Noc0, kernel_tensix, addr, value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut read = |addr| _chip.noc_read32(NocId::Noc0, kernel_tensix, addr);

        while read(buffer) != 1 {}

        println!("Started");

        let dma_addr = chip.pcie_access(paddr);
        write(buffer + (4 * 8), dma_addr as u32);
        write(buffer + (4 * 9), (dma_addr >> 32) as u32);
        write(buffer + (4 * 10), chip.pcie().into());
        let index = read(buffer + (4 * 4));

        write(buffer, 2);
        while read(buffer) != 3 {}

        println!("Waiting for end");

        kernel.wait();

        println!("Ended");

        let readback_value = u32::from_le_bytes([
            dma.buffer[offset as usize..][0],
            dma.buffer[offset as usize..][1],
            dma.buffer[offset as usize..][2],
            dma.buffer[offset as usize..][3],
        ]);

        let to_write_value = read(buffer + (index as u64 * 4));
        println!("READBACK[{index}]: {readback_value:x}");
        assert_eq!(
            to_write_value, readback_value,
            "{to_write_value:x} != {readback_value:x}"
        );
    }
}

#[test]
fn auto_aligned_dma_read_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let mut dma = chip.alloc_dma_aligned(1024, 64);
        dma.fill(0xa5);

        let mut kernel = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::{entry, target::noc_map::pci_read};

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 1024]> = NocAligned([0; 1024]);

                #[entry(brisc)]
                unsafe fn brisc_main() {
                    unsafe {
                        unsafe fn set_pc(pc: u16) {
                            unsafe {
                                tensix_std::set_postcode_brisc(0xc0de0000 | pc as u32);
                            }
                        }

                        let buf = &raw mut NOC_BUFFER.0[0];

                        let mut index = 14;
                        while buf.add(index) as usize % 64 != 0 {
                            index += 1;
                        }
                        buf.add(4).write_volatile(index as u32);

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        set_pc(0x101);

                        let noc_coords = buf.add(10).read_volatile();
                        let data = core::slice::from_raw_parts_mut(buf.add(index).cast::<u8>(), 4);
                        data.fill(0);
                        tensix_std::target::noc::noc_read(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: ((buf.add(9).read_volatile() as u64) << 32) | (buf.add(8).read_volatile() as u64),
                                x_end: noc_coords as u8,
                                y_end: (noc_coords >> 8) as u8,
                                ..Default::default()
                            },
                            data,
                            true,
                        );

                        set_pc(0x100);
                    }
                }
            }
        };

        let to_write_value = 0xfacau32;
        let value = to_write_value.to_le_bytes();

        dma[0] = value[0];
        dma[1] = value[1];
        dma[2] = value[2];
        dma[3] = value[3];

        let buffer = kernel["NOC_BUFFER"];

        println!("Waiting for start");

        let kernel_tensix = chip.tensix(0);

        let mut _chip = chip.dupe().unwrap();
        let mut write = |addr, value| {
            _chip.noc_write32(NocId::Noc0, kernel_tensix, addr, value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut read = |addr| _chip.noc_read32(NocId::Noc0, kernel_tensix, addr);

        while read(buffer) != 1 {}

        println!("Started");

        let dma_addr = chip.pcie_access(dma.physical_address());
        write(buffer + (4 * 8), dma_addr as u32);
        write(buffer + (4 * 9), (dma_addr >> 32) as u32);
        write(buffer + (4 * 10), chip.pcie().into());
        let index = read(buffer + (4 * 4));

        write(buffer, 2);
        while read(buffer) != 3 {}

        println!("Waiting for end");

        kernel.wait();

        println!("Ended");

        let readback_value = read(buffer + (index as u64 * 4));
        println!("READBACK[{index}]: {readback_value:x}");
        assert_eq!(
            to_write_value, readback_value,
            "{to_write_value:x} != {readback_value:x}"
        );
    }
}

#[test]
fn auto_alloc_dma_write_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        let mut dma = chip.alloc_dma_aligned(1024, 16);
        dma.fill(0xa5);

        let mut kernel = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(0),
            {
                use tensix_std::{entry, target::noc_map::pci_read};

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 1024]> = NocAligned([0; 1024]);

                #[entry(brisc)]
                unsafe fn brisc_main() {
                    unsafe {
                        unsafe fn set_pc(pc: u16) {
                            unsafe {
                                tensix_std::set_postcode_brisc(0xc0de0000 | pc as u32);
                            }
                        }

                        let buf = &raw mut NOC_BUFFER.0[0];

                        let mut index = 14;
                        while buf.add(index) as usize % 64 != 0 {
                            index += 1;
                        }
                        buf.add(4).write_volatile(index as u32);

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        set_pc(0x101);

                        buf.add(index).write_volatile(0xfaca);
                        let noc_coords = buf.add(10).read_volatile();
                        let data = core::slice::from_raw_parts_mut(buf.add(index).cast::<u8>(), 4);
                        tensix_std::target::noc::noc_write(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: ((buf.add(9).read_volatile() as u64) << 32) | (buf.add(8).read_volatile() as u64),
                                x_end: noc_coords as u8,
                                y_end: (noc_coords >> 8) as u8,
                                ..Default::default()
                            },
                            data,
                            true,
                        );

                        set_pc(0x100);
                    }
                }
            }
        };

        let buffer = kernel["NOC_BUFFER"];

        println!("Waiting for start");

        let kernel_tensix = chip.tensix(0);

        let mut _chip = chip.dupe().unwrap();
        let mut write = |addr, value| {
            _chip.noc_write32(NocId::Noc0, kernel_tensix, addr, value);
        };
        let mut _chip = chip.dupe().unwrap();
        let mut read = |addr| _chip.noc_read32(NocId::Noc0, kernel_tensix, addr);

        while read(buffer) != 1 {}

        println!("Started");

        let dma_addr = chip.pcie_access(dma.physical_address());
        write(buffer + (4 * 8), dma_addr as u32);
        write(buffer + (4 * 9), (dma_addr >> 32) as u32);
        write(buffer + (4 * 10), chip.pcie().into());
        let index = read(buffer + (4 * 4));

        write(buffer, 2);
        while read(buffer) != 3 {}

        println!("Waiting for end");

        kernel.wait();

        println!("Ended");

        let readback_value = u32::from_le_bytes([dma[0], dma[1], dma[2], dma[3]]);

        let to_write_value = read(buffer + (index as u64 * 4));
        println!("READBACK[{index}]: {readback_value:x}");
        assert_eq!(
            to_write_value, readback_value,
            "{to_write_value:x} != {readback_value:x}"
        );
    }
}

#[test]
fn tensix_to_dram_test() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = chip::open(id) {
            chip
        } else {
            continue;
        };

        if chip.arch().is_grayskull() {
            continue;
        }

        let mut kernel = rust_test! {
            nowait,
            chip,
            NocId::Noc0,
            chip.tensix(1),
            {
                use tensix_std::entry;

                #[unsafe(no_mangle)]
                pub static mut NOC_BUFFER: NocAligned<[u32; 128]> = NocAligned([0; 128]);

                #[entry(brisc)]
                unsafe fn entry() {
                    unsafe {
                        let buf = &raw mut NOC_BUFFER.0[0];

                        buf.write_volatile(1);
                        while buf.read_volatile() != 2 {}
                        buf.write_volatile(3);

                        let dram = Tile::from(buf.add(1).read_volatile());

                        let mut index = 14;
                        while buf.add(index) as usize % tensix_std::target::noc_map::ALIGNMENT_DRAM_WRITE != 0 {
                            index += 1;
                        }

                        buf.add(index).write_volatile(0xfaca);
                        tensix_std::target::noc::noc_write(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: 0,
                                x_end: dram.n0.0,
                                y_end: dram.n0.1,
                                ..Default::default()
                            },
                            core::slice::from_raw_parts(buf.add(index) as *const u8, 4),
                            true,
                        );

                        let mut index = 14;
                        while buf.add(index) as usize % tensix_std::target::noc_map::ALIGNMENT_DRAM_READ != 0 {
                            index += 1;
                        }

                        buf.add(index).write_volatile(0);
                        tensix_std::target::noc::noc_read(
                            tensix_std::target::noc::NocCommandSel::default(),
                            tensix_std::target::noc::NocAddr {
                                offset: 0,
                                x_end: dram.n0.0,
                                y_end: dram.n0.1,
                                ..Default::default()
                            },
                            core::slice::from_raw_parts_mut(buf.add(index) as *mut u8, 4),
                            true,
                        );

                        assert_eq!(0xfaca, buf.add(index).read_volatile());
                    }
                }
            }
        };

        let buffer = kernel["NOC_BUFFER"];

        while kernel.read32(buffer) != 1 {}

        kernel.write32(buffer + 4, chip.dram(0)[0].into());

        kernel.write32(buffer, 2);
        while kernel.read32(buffer) != 3 {}

        kernel.wait();

        let dram_data = chip.noc_read32(NocId::Noc0, chip.dram(0)[0], 0x0);
        assert_eq!(dram_data, 0xfaca);
    }
}
