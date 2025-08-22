use postcard::experimental::max_size::MaxSize;
use serde::{Deserialize, Serialize};

/// Info required to setup each core to execute the kernel workload.
#[derive(Default, Deserialize, Serialize)]
pub struct CoreLaunchData {
    /// Offset relative to where the workload is loaded to jump to in order to start executing
    pub entry: u32,
    /// Address in the core address map to set the stack pointer
    /// If it is None then the current stack will be used
    pub stack: Option<u32>,
}

impl CoreLaunchData {
    pub const fn cdefault() -> Self {
        Self {
            entry: 0,
            stack: None,
        }
    }
}

#[derive(Default, Deserialize, Serialize, MaxSize)]
pub struct LaunchRequest {
    pub job_request_addr: u64,
    pub job_offset: u64,
}

#[derive(Default, Deserialize, Serialize)]
pub struct LaunchData {
    pub workload_bank: (u8, u8),
    pub workload_bank_offset: u64,
    pub workload_bank_size: u64,

    pub bss: u32,
    pub ebss: u32,

    pub brisc: CoreLaunchData,
    pub ncrisc: CoreLaunchData,
    pub trisc0: CoreLaunchData,
    pub trisc1: CoreLaunchData,
    pub trisc2: CoreLaunchData,

    pub data_bank: (u8, u8),
    pub data_bank_offset: u64,

    /// Used to indicate that the launch data has been written into the core.
    /// Any non-zero value indicates presence of the data, will be written back to zero once
    /// the core has accepted the job.
    pub flag: u32,
}

impl LaunchData {
    pub const fn cdefault() -> Self {
        Self {
            workload_bank: (0, 0),
            workload_bank_offset: 0,
            workload_bank_size: 0,
            bss: 0,
            ebss: 0,
            brisc: CoreLaunchData::cdefault(),
            ncrisc: CoreLaunchData::cdefault(),
            trisc0: CoreLaunchData::cdefault(),
            trisc1: CoreLaunchData::cdefault(),
            trisc2: CoreLaunchData::cdefault(),
            data_bank: (0, 0),
            data_bank_offset: 0,
            flag: 0,
        }
    }
}

#[derive(Default)]
#[repr(C)]
pub struct CLaunchData {
    pub bss: u32,
    pub ebss: u32,

    pub brisc: CoreLaunchData,
    pub ncrisc: CoreLaunchData,
    pub trisc0: CoreLaunchData,
    pub trisc1: CoreLaunchData,
    pub trisc2: CoreLaunchData,

    /// Used to indicate that the launch data has been written into the core.
    /// Any non-zero value indicates presence of the data, will be written back to zero once
    /// the core has accepted the job.
    pub flag: u32,
}

impl CLaunchData {
    pub const fn cdefault() -> Self {
        Self {
            bss: 0,
            ebss: 0,
            brisc: CoreLaunchData::cdefault(),
            ncrisc: CoreLaunchData::cdefault(),
            trisc0: CoreLaunchData::cdefault(),
            trisc1: CoreLaunchData::cdefault(),
            trisc2: CoreLaunchData::cdefault(),
            flag: 0,
        }
    }
}
