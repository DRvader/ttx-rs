#[derive(Debug)]
#[repr(C)]
pub struct HostTensixCBHeader {
    write: u32,
    size: u32,
    read: u32,
    data_base: u32,
}

impl HostTensixCBHeader {
    pub fn push(&self) {}
}
