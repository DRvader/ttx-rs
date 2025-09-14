use super::Chip;

pub use ttkmd_if::DmaBuffer;

pub struct AlignedDmaBuffer {
    buffer: DmaBuffer,
    offset: usize,
    pub align: u32,
    pub size: usize,
}

impl std::ops::Index<std::ops::Range<usize>> for AlignedDmaBuffer {
    type Output = [u8];

    fn index(&self, index: std::ops::Range<usize>) -> &Self::Output {
        &self.buffer.buffer[self.offset..][index]
    }
}

impl std::ops::IndexMut<std::ops::Range<usize>> for AlignedDmaBuffer {
    fn index_mut(&mut self, index: std::ops::Range<usize>) -> &mut Self::Output {
        &mut self.buffer.buffer[self.offset..][index]
    }
}

impl std::ops::Index<usize> for AlignedDmaBuffer {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.buffer.buffer[self.offset + index]
    }
}

impl std::ops::IndexMut<usize> for AlignedDmaBuffer {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.buffer.buffer[self.offset + index]
    }
}

impl AlignedDmaBuffer {
    pub fn physical_address(&self) -> u64 {
        self.buffer.physical_address + self.offset as u64
    }

    pub fn fill(&mut self, value: u8) {
        self.buffer.buffer.fill(value);
    }

    pub fn ptr(&self) -> *const u8 {
        self.buffer.buffer.as_ptr()
    }

    pub fn mut_ptr(&mut self) -> *mut u8 {
        self.buffer.buffer.as_mut_ptr()
    }
}

impl Chip {
    pub fn alloc_dma(&mut self, size: usize) -> DmaBuffer {
        match self {
            Chip::Grayskull(grayskull) => grayskull
                .interface
                .device
                .allocate_dma_buffer(size as u32)
                .map_err(|v| v.to_string())
                .unwrap(),
            Chip::Wormhole(wormhole) => wormhole
                .interface
                .device
                .allocate_dma_buffer(size as u32)
                .map_err(|v| v.to_string())
                .unwrap(),
            Chip::Blackhole(blackhole) => blackhole
                .interface
                .device
                .allocate_dma_buffer(size as u32)
                .map_err(|v| v.to_string())
                .unwrap(),
        }
    }

    pub fn alloc_dma_aligned(&mut self, size: usize, align: u32) -> AlignedDmaBuffer {
        let actual_size = size + align as usize;

        let buffer = self.alloc_dma(actual_size);

        let big_align = align as u64;
        let aligned_addr = (buffer.physical_address + (big_align - 1)) & !(big_align - 1);
        let offset = aligned_addr - buffer.physical_address;

        AlignedDmaBuffer {
            buffer,
            offset: offset as usize,
            size: actual_size - offset as usize,
            align,
        }
    }
}
