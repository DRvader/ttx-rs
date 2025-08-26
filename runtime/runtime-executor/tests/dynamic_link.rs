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

fn load_push_firmware(chip: &mut Chip, parameters: PushFirmwareParameters) -> PushFirmware {
    let mut firmware = PushFirmware::compile(parameters, chip.arch());

    chip.load_kernels(&mut firmware.data, None, false);

    firmware
}

#[test]
fn dynamic_load_push() {
    for chip in ttx_rs::chip::scan() {
        if chip.is_err() {
            continue;
        }

        let mut chip = chip.unwrap();

        let firmware =
            load_push_firmware(&mut chip, PushFirmwareParameters { use_defmt: false });

        let builder = workload::WorkloadBuilder {
            available_space: firmware.available_space(),
            global: "#[unsafe(no_mangle)]\nstatic VALUE: SYNC<u32> = SYNC::new(26);".to_string(),
            brisc: "VALUE.write(2); while VALUE.read() == 1 {}".to_string(),
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

        let paramters = DramPullFirmwareParameters { use_defmt: false };
        let mut firmware = load_dram_pull_firmware(&mut chip, paramters);

        let mut builder = workload::WorkloadBuilder {
            available_space: firmware.available_space(),
            ..Default::default()
        };

        let buffer = builder.output_buffer("SYNC", 4, 1);
        let slot = buffer.get_slot(0).unwrap();

        builder.brisc = format!(
            r#"
            // Signal that the buffers are ready for being interacted with
            make_buffers_valid();

            // Blocks until flushed
            buffer_push(smallest_read_for_{smallest_function}, &{sync_data}, &{sync_write}, &0xfacau32.to_le_bytes());
            {sync_write}.write(4);
            // Blocks until buffer is flushed
            buffer_complete(smallest_read_for_{smallest_function}, &{sync_write}, &{sync_flush});
            "#,
            sync_data = slot.symbol_data(),
            sync_write = slot.symbol_write(),
            smallest_function = slot.symbol_base(),
            sync_flush = slot.symbol_flushed()
        );

        let workload = builder.compile(chip.arch());

        info!("COMPILED");

        let workload = firmware.queue_workload(&mut chip, workload, vec![buffer]);

        // let log_chip = chip.dupe().unwrap();
        // std::thread::spawn(move || {
        //     runtime_executor::firmware::defmt::run_defmt(
        //         log_chip,
        //         workload.tile,
        //         &firmware.elf,
        //         &firmware.data,
        //     );
        // });

        workload.wait_buffers_valid(&mut chip);

        let data = workload.empty_output(&mut chip, &slot);
        let data = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);

        assert_eq!(data, 0xfaca, "When testing {chip}: 0x{data:x}(actual) != 0xfaca(expected)");
    }
}
