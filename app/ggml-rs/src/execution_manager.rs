use runtime_executor::{
    firmware::dram_pull::{DramPullFirmware, DramPullFirmwareParameters, QueuedWorkload},
    workload::{OutputBuffer, WorkloadBuilder},
};
use ttx_rs::Chip;

pub struct ExecutionManager {
    pub chip: Chip,
    pub firmware: DramPullFirmware,
}

impl ExecutionManager {
    pub fn new(mut chip: Chip) -> ExecutionManager {
        let mut firmware =
            DramPullFirmware::compile(&mut chip, DramPullFirmwareParameters { use_defmt: false });

        chip.load_kernels(&mut firmware.data, None, false);

        ExecutionManager { chip, firmware }
    }

    pub fn queue(
        &mut self,
        outputs: Vec<(OutputBuffer, usize)>,
        mut kernel: WorkloadBuilder,
        buffers: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> QueuedWorkload {
        let workload = kernel.compile(self.chip.arch());
        self.firmware
            .queue_workload(&mut self.chip, workload, vec![])
    }
}
