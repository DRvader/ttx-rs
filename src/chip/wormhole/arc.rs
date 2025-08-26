use thiserror::Error;

use super::Wormhole;

#[derive(Debug, Clone, Copy)]
pub enum PowerState {
    Busy,
    ShortIdle,
    LongIdle,
}

#[derive(Debug)]
pub enum ArcState {
    A0,
    A1,
    A3,
    A5,
}

#[derive(Debug)]
pub enum FwType {
    ArcL2,
    FwBundle,
    FwBundleSPI,
}

#[derive(Debug)]
pub enum ArcMsg {
    Nop,
    Test { arg: u32 },
    ArcGoToSleep,

    SetPowerState(PowerState),

    FwVersion(FwType),
    GetSmbusTelemetryAddr,

    SetArcState { state: ArcState },

    ResetSafeClks { arg: u32 },
    ToggleTensixReset { arg: u32 },
    DeassertRiscVReset,
    GetAiclk,
    TriggerReset,
    GetHarvesting,
    TriggerSpiCopyLtoR,
    GetSpiDumpAddr,
    SpiRead { addr: u32 },
    SpiWrite,
    Raw { msg_code: u16, arg0: u16, arg1: u16 },
}

impl ArcMsg {
    pub fn msg_code(&self) -> u16 {
        let short_code = match self {
            ArcMsg::Nop => 0x11,
            ArcMsg::ArcGoToSleep => 0x55,
            ArcMsg::Test { .. } => 0x90,
            ArcMsg::GetSmbusTelemetryAddr => 0x2C,
            ArcMsg::TriggerSpiCopyLtoR => 0x50,
            ArcMsg::SetPowerState(state) => match state {
                PowerState::Busy => 0x52,
                PowerState::ShortIdle => 0x53,
                PowerState::LongIdle => 0x54,
            },
            ArcMsg::TriggerReset => 0x56,
            ArcMsg::GetHarvesting => 0x57,
            ArcMsg::DeassertRiscVReset => 0xba,
            ArcMsg::ResetSafeClks { .. } => 0xbb,
            ArcMsg::ToggleTensixReset { .. } => 0xaf,
            ArcMsg::GetAiclk => 0x34,
            ArcMsg::SetArcState { state } => match state {
                ArcState::A0 => 0xA0,
                ArcState::A1 => 0xA1,
                ArcState::A3 => 0xA3,
                ArcState::A5 => 0xA5,
            },
            ArcMsg::FwVersion(_) => 0xb9,
            ArcMsg::GetSpiDumpAddr => 0x29,
            ArcMsg::SpiRead { .. } => 0x2A,
            ArcMsg::SpiWrite => 0x2B,
            ArcMsg::Raw { msg_code, .. } => *msg_code,
        };

        0xaa00 | short_code
    }

    pub fn args(&self) -> (u16, u16) {
        match self {
            ArcMsg::Raw { arg0, arg1, .. } => (*arg0, *arg1),
            ArcMsg::Test { arg }
            | ArcMsg::ResetSafeClks { arg }
            | ArcMsg::ToggleTensixReset { arg }
            | ArcMsg::SpiRead { addr: arg } => {
                ((arg & 0xFFFF) as u16, ((arg >> 16) & 0xFFFF) as u16)
            }
            ArcMsg::SpiWrite => (0xFFFF, 0xFFFF),
            ArcMsg::Nop
            | ArcMsg::ArcGoToSleep
            | ArcMsg::GetSmbusTelemetryAddr
            | ArcMsg::SetPowerState(_)
            | ArcMsg::DeassertRiscVReset
            | ArcMsg::GetAiclk
            | ArcMsg::TriggerReset
            | ArcMsg::GetHarvesting
            | ArcMsg::GetSpiDumpAddr
            | ArcMsg::TriggerSpiCopyLtoR
            | ArcMsg::SetArcState { .. } => (0, 0),
            ArcMsg::FwVersion(ty) => match ty {
                FwType::ArcL2 => (0, 0),
                FwType::FwBundle => (1, 0),
                FwType::FwBundleSPI => (2, 0),
            },
        }
    }

    pub fn from_values(msg: u16, arg0: u16, arg1: u16) -> Self {
        let arg = ((arg1 as u32) << 16) | arg0 as u32;
        match msg {
            0x11 => ArcMsg::Nop,
            0x34 => ArcMsg::GetAiclk,
            0x56 => ArcMsg::TriggerReset,
            0xbb => ArcMsg::ResetSafeClks { arg },
            0xaf => ArcMsg::ToggleTensixReset { arg },
            0xba => ArcMsg::DeassertRiscVReset,
            0x50 => ArcMsg::TriggerSpiCopyLtoR,
            0x52 => ArcMsg::SetPowerState(PowerState::Busy),
            0x53 => ArcMsg::SetPowerState(PowerState::ShortIdle),
            0x54 => ArcMsg::SetPowerState(PowerState::LongIdle),
            0x57 => ArcMsg::GetHarvesting,
            0x90 => ArcMsg::Test { arg },
            0xA0 => ArcMsg::SetArcState {
                state: ArcState::A0,
            },
            0xA1 => ArcMsg::SetArcState {
                state: ArcState::A1,
            },
            0xA3 => ArcMsg::SetArcState {
                state: ArcState::A3,
            },
            0xA5 => ArcMsg::SetArcState {
                state: ArcState::A5,
            },
            0xB9 => ArcMsg::FwVersion(match arg {
                0 => FwType::ArcL2,
                1 => FwType::FwBundle,
                2 => FwType::FwBundleSPI,
                _ => panic!("Unknown FW type {arg}"),
            }),
            value => ArcMsg::Raw {
                msg_code: value,
                arg0,
                arg1,
            },
        }
    }
}

#[derive(Debug)]
pub struct BtWrapper(pub std::backtrace::Backtrace);

impl BtWrapper {
    #[inline(always)]
    pub fn capture() -> Self {
        Self(std::backtrace::Backtrace::capture())
    }
}

impl std::fmt::Display for BtWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let std::backtrace::BacktraceStatus::Captured = self.0.status() {
            self.0.fmt(f)?;
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum ArcMsgProtocolError {
    #[error("Message {0} not recognized")]
    MsgNotRecognized(u16),
    #[error("Timed out while waiting {0:?} for ARC to respond")]
    Timeout(std::time::Duration),
    #[error("ARC is asleep")]
    ArcAsleep,
    #[error("Failed to trigger FW interrupt")]
    FwIntFailed,
    #[error("Mailbox {0} is invalid")]
    InvalidMailbox(usize),
    #[error("Unknown error code {0}")]
    UnknownErrorCode(u8),
}

impl ArcMsgProtocolError {
    #[inline(always)]
    pub fn into_error(self) -> ArcMsgError {
        ArcMsgError::ProtocolError {
            source: self,
            backtrace: BtWrapper(std::backtrace::Backtrace::capture()),
        }
    }
}

#[derive(Error, Debug)]
pub enum ArcMsgError {
    #[error("{source}\n{backtrace}")]
    ProtocolError {
        source: ArcMsgProtocolError,
        backtrace: BtWrapper,
    },

    #[error(transparent)]
    AxiError(#[from] luwen::ttkmd_if::PciError),
}

#[derive(Debug)]
pub enum ArcMsgOk {
    Ok { rc: u32, arg: u32 },
    OkNoWait,
}

impl ArcMsgOk {
    pub fn arg(&self) -> u32 {
        match self {
            ArcMsgOk::Ok { arg, .. } => *arg,
            ArcMsgOk::OkNoWait => 0,
        }
    }

    pub fn rc(&self) -> u32 {
        match self {
            ArcMsgOk::Ok { rc, .. } => *rc,
            ArcMsgOk::OkNoWait => 0,
        }
    }
}

fn arc_read32(chip: &mut Wormhole, addr: u32) -> Result<u32, luwen::ttkmd_if::PciError> {
    chip.interface.device.read32(addr)
}

fn arc_write32(
    chip: &mut Wormhole,
    addr: u32,
    value: u32,
) -> Result<(), luwen::ttkmd_if::PciError> {
    chip.interface.device.write32(addr, value)
}

/// Returns True if new interrupt triggered, or False if the
/// FW is currently busy. The message IRQ handler should only take a couple
/// dozen cycles, so if this returns False it probably means something went
/// wrong.
fn trigger_fw_int(chip: &mut Wormhole, addrs: &ArcMsgAddr) -> Result<bool, ArcMsgError> {
    let misc = arc_read32(chip, addrs.arc_misc_cntl)?;

    if misc & (1 << 16) != 0 {
        return Ok(false);
    }

    let misc_bit16_set = misc | (1 << 16);
    arc_write32(chip, addrs.arc_misc_cntl, misc_bit16_set)?;

    Ok(true)
}

#[derive(Clone, Debug)]
pub struct ArcMsgAddr {
    pub scratch_base: u32,
    pub arc_misc_cntl: u32,
}

pub fn arc_msg(
    chip: &mut Wormhole,
    msg: &ArcMsg,
    wait_for_done: bool,
    timeout: std::time::Duration,
    msg_reg: u32,
    return_reg: u32,
    addrs: &ArcMsgAddr,
) -> Result<ArcMsgOk, ArcMsgError> {
    const MSG_ERROR_REPLY: u32 = 0xffffffff;

    let (arg0, arg1) = msg.args();

    let code = msg.msg_code();

    let current_code = arc_read32(chip, addrs.scratch_base + (msg_reg * 4))?;
    if (current_code & 0xFFFF) as u16 == ArcMsg::ArcGoToSleep.msg_code() {
        Err(ArcMsgProtocolError::ArcAsleep.into_error())?;
    }

    arc_write32(
        chip,
        addrs.scratch_base + (return_reg * 4),
        arg0 as u32 | ((arg1 as u32) << 16),
    )?;

    arc_write32(chip, addrs.scratch_base + (msg_reg * 4), code as u32)?;

    if !trigger_fw_int(chip, addrs)? {
        return Err(ArcMsgProtocolError::FwIntFailed.into_error());
    }

    if wait_for_done {
        let start = std::time::Instant::now();
        loop {
            let status = arc_read32(chip, addrs.scratch_base + (msg_reg * 4))?;
            if (status & 0xFFFF) as u16 == code & 0xFF {
                let exit_code = (status >> 16) & 0xFFFF;
                let arg = arc_read32(chip, addrs.scratch_base + (return_reg * 4))?;

                return Ok(ArcMsgOk::Ok { rc: exit_code, arg });
            } else if status == MSG_ERROR_REPLY {
                return Err(ArcMsgProtocolError::MsgNotRecognized(code).into_error());
            }

            std::thread::sleep(std::time::Duration::from_millis(1));
            if start.elapsed() > timeout {
                return Err(ArcMsgProtocolError::Timeout(timeout).into_error());
            }
        }
    }

    Ok(ArcMsgOk::OkNoWait)
}
