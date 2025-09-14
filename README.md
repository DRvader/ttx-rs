# TTX-rs

All in one package for the host-side of executing code on Tenstorrent devices.

# Overview

- A chip abstraction providing PCIe access to chip resources
  - Parses harvesting information to provide access to only available tiles
  - Talks to power management fw to raise and lower clocks
  - Manages communication resources to allow easy and efficient access to the hardware
- Rust compiler interface allowing code to be built that can run on Tensix cores using a naitive compiler toolchain
  - Custom targets for grayskull, wormhole and blackhole
  - Loader that can write created binaries to the tensix and monitor their execution
  - Utility functions and macros allowing one crate to both contain host-side and tensix-side code
  - Utility functions allowing dynamically created rust code to be compiled and loaded
- Reference implementation of fw that allows smaller workloads to asyncronously sent to cores and executed

# Example

The simplest example will just compile a binary that immediately exits

Add the following to your Cargo.toml

```toml
[target.'cfg(target_vendor = "tenstorrent")'.dependencies]
tensix-std = { path = "<path to tensix-std>" }

[target.'cfg(not(target_vendor = "tenstorrent"))'.dependencies]
ttx-rs = { path = "<path to ttx-rs>" }
```

Then in the main.rs file

```rust
#![cfg_attr(target_vendor = "tenstorrent", no_std)]
#![cfg_attr(target_vendor = "tenstorrent", no_main)]

#[cfg(target_vendor = "tenstorrent")]
mod kernel {
    use tensix_std::entry;

    #[entry(brisc)]
    unsafe fn entry() {}
}

#[cfg(not(target_vendor = "tenstorrent"))]
fn run(index: usize) {
    // Compile the current crate for the tensix and load it into tensix[0]
    // Then load it and wait for completion
    ttx_rs::load!("kernel", device, device.tensix(0));
}

#[cfg(not(target_vendor = "tenstorrent"))]
fn main() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = crate::ttchip::open(id) {
            chip
        } else {
            continue;
        };

        run(chip);
    }
}
```

To do something a little more interesting we can create a kernel that will only complete when it recieves input from the host

```rust
#![cfg_attr(target_vendor = "tenstorrent", no_std)]
#![cfg_attr(target_vendor = "tenstorrent", no_main)]

#[cfg(target_vendor = "tenstorrent", kernel_name = "a")]
mod kernel {
    use tensix_std::entry;

    #[unsafe(no_mangle)]
    pub static mut NOC_BUFFER: NocAligned<[u32; 128]> = NocAligned([0; 128]);

    #[entry(brisc)]
    unsafe fn brisc_main() {
        unsafe {
            let buf = &raw mut NOC_BUFFER.0[0];

            // This is where the value will be written
            buf.add(1).write_volatile(0);

            // Sync point to ensure that the value in buf[1] is 0
            buf.write_volatile(1);
            while buf.read_volatile() != 2 {}
            buf.write_volatile(3);

            while buf.add(1).read_volatile() != 0xfaca {}
        }
    }
}

#[cfg(not(target_vendor = "tenstorrent"))]
fn run(index: usize) {
    // Again and compile the kernel for tensix[0] with two changes.
    // 1. The kernel name is now a, this means that only kernels tagged with the cfg "a" are compiled
    //    this is useful when compiling multiple kernels in the same file.
    // 2. The wait parameter and is set to false which means that as soon as the kernel is loaded the load! macro will return
    let kernel = ttx_rs::load!("a", device, device.tensix(0), wait = false);
    let buffer = kernel.data.sym_table["NOC_BUFFER"];

    // The Kernel type provides a few convinence functions for easily interacting with the core
    kernel.write32(buffer + 4, 0xfaca);

    // Wait for the kernel to complete
    kernel.wait();
}

#[cfg(not(target_vendor = "tenstorrent"))]
fn main() {
    for id in PciDevice::scan() {
        let mut chip = if let Ok(chip) = crate::ttchip::open(id) {
            chip
        } else {
            continue;
        };

        run(chip);
    }
}
```
