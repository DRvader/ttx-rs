use crate::chip::{
    field::Field,
    noc::{NocAddress, NocId},
};

use super::{pci_noc::PciNoc, Blackhole};

#[derive(thiserror::Error, Debug)]
pub enum ProtocolErrorType {
    #[error("Message code not recognized {0:x}")]
    MsgNotRecognized(u32),
    #[error("While processing message hit error {0}")]
    UnknownErrorCode(u8),
}

#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("Timed out in {phase} after {}s", .timeout.as_secs_f32())]
    Timeout {
        phase: String,
        timeout: std::time::Duration,
    },
    #[error("Selected out of range queue ({index} > {queue_count})")]
    QueueIndexOutOfRange { index: u32, queue_count: u32 },

    #[error("ProtocolError")]
    ProtocolError(ProtocolErrorType),

    #[error(transparent)]
    AxiError(#[from] luwen::ttkmd_if::PciError),
}

#[derive(Clone)]
pub struct MessageQueue<const N: usize> {
    pub header_size: u32,
    pub entry_size: u32,

    pub queue_base: u64,
    pub queue_count: u32,

    pub queue_size: u32,

    pub fw_int: Field,
}

fn arc_read(
    chip: &mut Blackhole,
    addr: u64,
    data: &mut [u8],
) -> Result<(), luwen::ttkmd_if::PciError> {
    chip.interface
        .tile_read(NocId::Noc0, chip.endpoints.arc.into(), addr, data)
}

fn arc_read32(chip: &mut Blackhole, addr: u64) -> Result<u32, luwen::ttkmd_if::PciError> {
    chip.interface
        .tile_read32(NocId::Noc0, chip.endpoints.arc.into(), addr)
}

fn arc_write(
    chip: &mut Blackhole,
    addr: u64,
    data: &[u8],
) -> Result<(), luwen::ttkmd_if::PciError> {
    chip.interface
        .tile_write(NocId::Noc0, chip.endpoints.arc.into(), addr, data)
}

fn arc_write32(
    chip: &mut Blackhole,
    addr: u64,
    value: u32,
) -> Result<(), luwen::ttkmd_if::PciError> {
    chip.interface
        .tile_write32(NocId::Noc0, chip.endpoints.arc.into(), addr, value)
}

impl<const N: usize> MessageQueue<N> {
    fn get_base(&self, index: u8) -> u64 {
        let msg_queue_size = 2 * self.queue_size * (self.entry_size * 4) + (self.header_size * 4);
        self.queue_base + (index as u64 * msg_queue_size as u64)
    }

    fn qread32(&self, chip: &mut Blackhole, index: u8, offset: u32) -> Result<u32, MessageError> {
        Ok(arc_read32(
            chip,
            self.get_base(index) + (4 * offset as u64),
        )?)
    }

    fn qwrite32(
        &self,
        chip: &mut Blackhole,
        index: u8,
        offset: u32,
        value: u32,
    ) -> Result<(), MessageError> {
        Ok(arc_write32(
            chip,
            self.get_base(index) + (4 * offset as u64),
            value,
        )?)
    }

    fn trigger_int(&self, chip: &mut Blackhole) -> Result<bool, MessageError> {
        let mut mvalue = vec![0u8; self.fw_int.size];
        let value = crate::chip::field::read_field(
            chip,
            |chip, addr, data| arc_read(chip, addr, data).unwrap(),
            self.fw_int,
            &mut mvalue,
        )
        .unwrap();

        if value[0] & 1 != 0 {
            return Ok(false);
        }

        mvalue[0] |= 1;

        crate::chip::field::write_field_vec(
            chip,
            |chip, addr, data| arc_read(chip, addr, data).unwrap(),
            |chip, addr, data| arc_write(chip, addr, data).unwrap(),
            self.fw_int,
            mvalue.as_slice(),
        );

        Ok(true)
    }

    fn push_request(
        &self,
        chip: &mut Blackhole,
        index: u8,
        request: &[u32; N],
        timeout: std::time::Duration,
    ) -> Result<(), MessageError> {
        let request_queue_wptr = self.qread32(chip, index, 0)?;

        let start_time = std::time::Instant::now();
        loop {
            let request_queue_rptr = self.qread32(chip, index, 4)?;

            // Check if the queue is full
            if request_queue_rptr.abs_diff(request_queue_wptr) % (2 * self.queue_size)
                != self.queue_size
            {
                break;
            }

            let elapsed = start_time.elapsed();
            if elapsed > timeout {
                return Err(MessageError::Timeout {
                    phase: "push".to_string(),
                    timeout: elapsed,
                })?;
            }
        }

        let request_entry_offset =
            self.header_size + (request_queue_wptr % self.queue_size) * N as u32;
        for (i, request) in request.iter().enumerate() {
            self.qwrite32(chip, index, request_entry_offset + i as u32, *request)?;
        }

        let request_queue_wptr = (request_queue_wptr + 1) % (2 * self.queue_size);
        self.qwrite32(chip, index, 0, request_queue_wptr)?;

        self.trigger_int(chip)?;

        Ok(())
    }

    fn pop_response(
        &self,
        chip: &mut Blackhole,
        index: u8,
        result: &mut [u32; N],
        timeout: std::time::Duration,
    ) -> Result<(), MessageError> {
        let response_queue_rptr = self.qread32(chip, index, 1)?;

        let start_time = std::time::Instant::now();
        loop {
            let response_queue_wptr = self.qread32(chip, index, 5)?;

            // Break if there is some data in the queue
            if response_queue_wptr != response_queue_rptr {
                break;
            }

            let elapsed = start_time.elapsed();
            if elapsed > timeout {
                return Err(MessageError::Timeout {
                    phase: "pop".to_string(),
                    timeout: elapsed,
                })?;
            }
        }

        let response_entry_offset = self.header_size
            + (self.queue_size + (response_queue_rptr % self.queue_size)) * N as u32;
        for (i, result) in result.iter_mut().enumerate() {
            *result = self.qread32(chip, index, response_entry_offset + i as u32)?;
        }

        let response_queue_rptr = (response_queue_rptr + 1) % (2 * self.queue_size);
        self.qwrite32(chip, index, 1, response_queue_rptr)?;

        Ok(())
    }

    pub fn send_message(
        &self,
        chip: &mut Blackhole,
        index: u8,
        mut request: [u32; N],
        timeout: std::time::Duration,
    ) -> Result<[u32; N], MessageError> {
        let mut lock = crate::chip::ARC_LOCK.lock().unwrap();
        while lock.len() <= chip.interface.device.id {
            lock.push(std::sync::Mutex::new(()));
        }

        let _lock = lock[chip.interface.device.id].lock();
        if index as u32 > self.queue_count {
            return Err(MessageError::QueueIndexOutOfRange {
                index: index as u32,
                queue_count: self.queue_count,
            })?;
        }

        self.push_request(chip, index, &request, timeout)?;
        self.pop_response(chip, index, &mut request, timeout)?;

        Ok(request)
    }
}

#[derive(Debug)]
pub struct QueueInfo {
    pub req_rptr: u32,
    pub req_wptr: u32,
    pub resp_rptr: u32,
    pub resp_wptr: u32,
}

impl<const N: usize> MessageQueue<N> {
    pub fn get_queue_info(
        &self,
        chip: &mut Blackhole,
        index: u8,
    ) -> Result<QueueInfo, MessageError> {
        if index as u32 > self.queue_count {
            return Err(MessageError::QueueIndexOutOfRange {
                index: index as u32,
                queue_count: self.queue_count,
            })?;
        }

        Ok(QueueInfo {
            req_rptr: self.qread32(chip, index, 4)?,
            req_wptr: self.qread32(chip, index, 0)?,
            resp_rptr: self.qread32(chip, index, 1)?,
            resp_wptr: self.qread32(chip, index, 5)?,
        })
    }
}

#[derive(Debug, PartialEq)]
#[repr(u8)]
pub enum ArcFwInitStatus {
    NotStarted = 0,
    Started = 1,
    Done = 2,
    Error = 3,
    Unknown(u8),
}

impl From<u8> for ArcFwInitStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => ArcFwInitStatus::NotStarted,
            1 => ArcFwInitStatus::Started,
            2 => ArcFwInitStatus::Done,
            3 => ArcFwInitStatus::Error,
            other => ArcFwInitStatus::Unknown(other),
        }
    }
}

impl ArcFwInitStatus {
    pub fn ready(&self) -> bool {
        match self {
            ArcFwInitStatus::NotStarted
            | ArcFwInitStatus::Started
            | ArcFwInitStatus::Unknown(_) => false,
            ArcFwInitStatus::Done | ArcFwInitStatus::Error => true,
        }
    }
}

pub fn arc_fw_init_status(chip: &mut PciNoc, arc: NocAddress) -> Option<ArcFwInitStatus> {
    chip.tile_read32(NocId::Noc0, arc, 0x80030000 + 0x400 + (4 * 2))
        .ok()
        .map(|boot_status_0| ArcFwInitStatus::from(((boot_status_0 >> 1) & 0x3) as u8))
}

pub fn check_arc_msg_safe(chip: &mut Blackhole) -> bool {
    // Note that hw_ready can be false while we can safely send an arc_msg
    // This confuses me a bit because this means you can send arc messages that will potentially poke an uninitialized hw
    if let Ok(boot_status_0) = arc_read32(chip, 0x80030000 + 0x400 + (4 * 2)) {
        (boot_status_0 & 0x1) == 1
    } else {
        false
    }
}

pub fn send_arc_msg(
    chip: &mut Blackhole,
    msg_id: u32,
    request: Option<[u32; 7]>,
) -> Result<(u8, u16, [u32; 7]), MessageError> {
    assert!(check_arc_msg_safe(chip));

    let message_queue_info_address = arc_read32(chip, 0x80030000 + 0x400 + (4 * 11))?;
    let queue_base = arc_read32(chip, message_queue_info_address as u64)?;
    let queue_sizing = arc_read32(chip, message_queue_info_address as u64 + 4)?;
    let queue_size = queue_sizing & 0xFF;
    let queue_count = (queue_sizing >> 8) & 0xFF;

    let queue = MessageQueue {
        header_size: 8,
        entry_size: 8,
        queue_base: queue_base as u64,
        queue_size,
        queue_count,
        fw_int: Field {
            addr: 0x80030000 + 0x100,
            size: 4,
            bits: Some((16, 19)),
        },
    };

    let mut actual_request = [0; 8];
    actual_request[0] = msg_id;
    if let Some(request) = request {
        for (a, b) in request.iter().copied().zip(actual_request[1..].iter_mut()) {
            *b = a;
        }
    }

    let response = queue.send_message(
        chip,
        2,
        actual_request,
        std::time::Duration::from_millis(500),
    )?;
    let status = (response[0] & 0xFF) as u8;
    let rc = (response[0] >> 16) as u16;

    if status < 240 {
        let data = [
            response[1],
            response[2],
            response[3],
            response[4],
            response[5],
            response[6],
            response[7],
        ];
        Ok((status, rc, data))
    } else if status == 0xFF {
        Err(MessageError::ProtocolError(
            ProtocolErrorType::MsgNotRecognized(msg_id),
        ))
    } else {
        Err(MessageError::ProtocolError(
            ProtocolErrorType::UnknownErrorCode(status),
        ))
    }
}
