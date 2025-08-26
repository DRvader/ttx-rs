#![no_std]

use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize};

/// The value to substitute on the right side of the
/// relocation operation
#[derive(Clone, Serialize, Deserialize, MaxSize)]
pub enum RelocationRead {
    /// Instead of reading substitute the loaded binary base
    Base32,
    /// Perform a relocation based on the symbol value
    /// This implies that the symbol value should have the loaded base
    /// added
    SymbolValue32(u32),
    /// Offset from the loaded base to read from
    Offset(u64),
}

/// We only support loading self contained kernels and no direct communication
/// between the dynamic file and the firmware.
/// We also load all sections as a single block
/// This means that we, in advance, know where symbols where be located relative to a global base
#[derive(Clone, Serialize, Deserialize, MaxSize)]
pub struct KernelRelocation {
    /// The offset relative to the bottom of the binary where the write needs to happen
    pub write_offset: u64,
    /// The offset relative to the bottom of the binary where the read needs to happen
    pub read_offset: RelocationRead,
    /// A constant value to add to the result of the read
    pub addend: i64,
}

impl KernelRelocation {
    /// # Safety
    ///
    /// Please make sure the binary pointer is valid.
    pub unsafe fn relocate_ptr(&self, base_addr: u64, mut binary: *mut u8) {
        self.relocate(
            &mut binary,
            base_addr,
            |binary, addr| {
                let read_value = unsafe { binary.add(addr) };
                unsafe {
                    u32::from_le_bytes([
                        read_value.read_volatile(),
                        read_value.add(1).read_volatile(),
                        read_value.add(2).read_volatile(),
                        read_value.add(3).read_volatile(),
                    ])
                }
            },
            |binary, addr, value| {
                let write_loc = unsafe { binary.add(addr) };

                unsafe {
                    write_loc.write_volatile(value[0]);
                    write_loc.add(1).write_volatile(value[1]);
                    write_loc.add(2).write_volatile(value[2]);
                    write_loc.add(3).write_volatile(value[3]);
                }
            },
        )
    }

    pub fn relocate_binary(&self, base_addr: u64, binary: &mut [u8]) {
        self.relocate(
            binary,
            base_addr,
            |binary, addr| {
                let read_value = &binary[addr..];
                u32::from_le_bytes([read_value[0], read_value[1], read_value[2], read_value[3]])
            },
            |binary, addr, value| {
                let write_loc = &mut binary[addr..];

                write_loc[0] = value[0];
                write_loc[1] = value[1];
                write_loc[2] = value[2];
                write_loc[3] = value[3];
            },
        )
    }

    pub fn relocate<T: ?Sized>(
        &self,
        data: &mut T,
        base_addr: u64,
        read32: impl FnOnce(&mut T, usize) -> u32,
        write32: impl FnOnce(&mut T, usize, [u8; 4]),
    ) {
        let value = match self.read_offset {
            RelocationRead::Base32 => base_addr as u32,
            RelocationRead::SymbolValue32(value) => base_addr as u32 + value,
            RelocationRead::Offset(read_offset) => (read32)(data, read_offset as usize),
        };

        let value = ((value as i64 + self.addend) as u32).to_le_bytes();
        (write32)(data, self.write_offset as usize, value);
    }
}
