use std::collections::BTreeMap;

#[derive(Debug)]
struct FreeRegion {
    offset: usize,
    size: usize,
}

#[derive(Debug)]
struct Buffer {
    size: usize,
    free_regions: BTreeMap<usize, usize>, // offset -> size
}

impl Buffer {
    fn new(size: usize) -> Self {
        let mut free_regions = BTreeMap::new();
        free_regions.insert(0, size);
        Buffer { size, free_regions }
    }

    fn allocate(&mut self, size: usize, align: usize) -> Option<usize> {
        for (&offset, &region_size) in &self.free_regions {
            let aligned_offset = (offset + align - 1) & !(align - 1);
            let padding = aligned_offset - offset;
            if region_size >= padding + size {
                // Remove or shrink the current region
                self.free_regions.remove(&offset);
                let remaining = region_size - (padding + size);
                if padding > 0 {
                    self.free_regions.insert(offset, padding);
                }
                if remaining > 0 {
                    self.free_regions.insert(aligned_offset + size, remaining);
                }
                return Some(aligned_offset);
            }
        }
        None
    }

    fn free(&mut self, offset: usize, size: usize) {
        // Coalesce with adjacent regions
        let mut start = offset;
        let mut end = offset + size;

        if let Some((&prev_offset, &prev_size)) = self.free_regions.range(..offset).next_back() {
            if prev_offset + prev_size == offset {
                start = prev_offset;
                end = offset + size;
                self.free_regions.remove(&prev_offset);
            }
        }

        if let Some((&next_offset, &next_size)) = self.free_regions.range(offset..).next() {
            if offset + size == next_offset {
                end = next_offset + next_size;
                self.free_regions.remove(&next_offset);
            }
        }

        self.free_regions.insert(start, end - start);
    }
}

#[derive(Debug)]
pub struct MultiBufferAllocator {
    buffers: Vec<Buffer>,
}

pub struct Allocation {
    pub dram_bank: usize,
    pub offset: u64,
}

impl MultiBufferAllocator {
    pub fn new(buffer_sizes: Vec<usize>) -> Self {
        let buffers = buffer_sizes.into_iter().map(Buffer::new).collect();
        MultiBufferAllocator { buffers }
    }

    /// Allocates `size` bytes with `align` alignment.
    /// Returns (buffer_index, offset) on success.
    pub fn allocate(&mut self, size: usize, align: usize) -> Option<Allocation> {
        for (i, buffer) in self.buffers.iter_mut().enumerate() {
            if let Some(offset) = buffer.allocate(size, align) {
                return Some(Allocation {
                    dram_bank: i,
                    offset: offset as u64,
                });
            }
        }
        None
    }

    /// Frees a previously allocated block.
    pub fn free(&mut self, buffer_index: usize, offset: usize, size: usize) {
        if let Some(buffer) = self.buffers.get_mut(buffer_index) {
            buffer.free(offset, size);
        }
    }
}
