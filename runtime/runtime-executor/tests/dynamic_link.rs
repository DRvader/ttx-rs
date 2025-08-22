use runtime_executor::workload;
use tracing::info;
use ttx_rs::{Chip, chip::noc::NocInterface};

use runtime_executor::firmware::{
    dram_pull::{DramPullFirmware, DramPullFirmwareParameters},
    push::{PushFirmware, PushFirmwareParameters},
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

fn load_push_firmware(chip: &mut Chip) -> PushFirmware {
    let mut firmware = PushFirmware::compile(PushFirmwareParameters {}, chip.arch());

    chip.load_kernels(&mut firmware.data, None, false);

    firmware
}

#[test]
fn quick_dynamic_load() {
    for chip in ttx_rs::chip::scan() {
        if chip.is_err() {
            continue;
        }

        let mut chip = chip.unwrap();

        let firmware = load_push_firmware(&mut chip);

        let builder = workload::WorkloadBuilder {
            available_space: firmware.available_space(),
            global: "#[unsafe(no_mangle)]\nstatic VALUE: SYNC<u32> = SYNC::new(26);".to_string(),
            brisc: "VALUE.write(2); while VALUE.read() == 1 {}".to_string(),
            ncrisc: "while VALUE.read() == 26 {}".to_string(),
            ..Default::default()
        };

        let workload = builder.compile(chip.arch());

        info!("COMPILED");

        let loaded = firmware.push_workload(&mut chip, workload);

        info!("PUSHED");

        firmware.wait(&mut chip, loaded.tile);

        info!("VALUE: {:x} {:x}", loaded.base_addr, loaded.data("VALUE"));

        let result = chip.noc_read32(
            ttx_rs::chip::noc::NocId::Noc1,
            loaded.tile,
            loaded.data("VALUE"),
        );

        assert_eq!(result, 2);
    }
}

fn load_dram_pull_firmware(
    chip: &mut Chip,
    parameters: DramPullFirmwareParameters,
) -> DramPullFirmware {
    let mut firmware = DramPullFirmware::compile(chip, parameters);

    chip.load_kernels(&mut firmware.data, None, false);

    firmware
}

#[test]
fn dynamic_load_dram_pull() {
    for chip in ttx_rs::chip::scan() {
        if chip.is_err() {
            continue;
        }

        let mut chip = chip.unwrap();

        let need_job_request = chip.alloc_dma_aligned(64 * chip.tensix_count(), 64);

        let paramters = DramPullFirmwareParameters {
            job_server: chip.pcie(),
            job_server_addr: chip.pcie_access(need_job_request.physical_address()),
        };
        let mut firmware = load_dram_pull_firmware(&mut chip, paramters);

        let builder = workload::WorkloadBuilder {
            available_space: firmware.available_space(),
            global: "#[unsafe(no_mangle)]\nstatic VALUE: SYNC<u32> = SYNC::new(26);".to_string(),
            brisc: "VALUE.write(2); while VALUE.read() == 1 {}".to_string(),
            ncrisc: "while VALUE.read() == 26 {}".to_string(),
            ..Default::default()
        };

        let workload = builder.compile(chip.arch());

        info!("COMPILED");

        firmware.queue_workload(&mut chip, workload, vec![], vec![]);
    }
}
