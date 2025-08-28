fn wrap(value: u32, size: u32) -> u32 {
    let value = value as i64 % size as i64 * 2;
    let value = if value < 0 {
        value + size as i64 * 2
    } else {
        value
    };

    value as u32
}

pub trait CbObserver {
    fn get_read(&self) -> u32;
    fn get_write(&self) -> u32;
    fn get_capacity(&self) -> u32;

    fn empty(read: u32, write: u32) -> bool {
        read == write
    }

    fn full(read: u32, write: u32, capacity: u32) -> bool {
        Self::size(read, write, capacity) == capacity
    }

    fn size(read: u32, write: u32, capacity: u32) -> u32 {
        wrap(write - read, capacity)
    }

    fn free(read: u32, write: u32, capacity: u32) -> u32 {
        capacity - Self::size(read, write, capacity)
    }

    fn read_size(&self) -> u32 {
        Self::size(self.get_read(), self.get_write(), self.get_capacity())
    }
}

pub trait CbProducer: CbObserver {
    fn set_write(&self, value: u32);
    fn set_data(&self, offset: u32, value: &[u8]);

    fn push(&self, input: &[u8]) -> u32 {
        let capacity = self.get_capacity();
        let read = self.get_read();
        let write = self.get_write();

        if Self::full(read, write, capacity) {
            return 0;
        }

        let size = Self::size(read, write, capacity);

        let count = (input.len() as u32).min(capacity - size);

        let first_write = count.min(capacity - (write % capacity)) as usize;

        self.set_data(write % capacity, &input[..first_write]);
        self.set_data(0, &input[first_write..]);

        self.set_write(wrap(write + count, capacity));

        count
    }

    fn push_all(&self, mut input: &[u8]) {
        while !input.is_empty() {
            input = &input[self.push(input) as usize..];
        }
    }
}

pub trait CbConsumer: CbObserver {
    fn set_read(&self, value: u32);
    fn get_data(&self, offset: u32, data: &mut [u8]);

    fn pop(&self, output: &mut [u8]) -> u32 {
        let capacity = self.get_capacity();
        let write = self.get_write();
        let read = self.get_read();

        if Self::empty(read, write) {
            return 0;
        }

        let size = Self::size(read, write, capacity);

        let count = (output.len() as u32).min(size);

        let first_read = count.min(capacity - (read % capacity)) as usize;

        self.get_data(read % capacity, &mut output[..first_read]);
        self.get_data(0, &mut output[first_read..]);

        self.set_read(wrap(read + count, capacity));

        count
    }

    fn pop_all(&self, mut output: &mut [u8]) {
        while !output.is_empty() {
            let popped = self.pop(output);
            output = &mut output[popped as usize..];
        }
    }
}

pub trait CbObserverMut {
    fn get_read_mut(&mut self) -> u32;
    fn get_write_mut(&mut self) -> u32;
    fn get_capacity_mut(&mut self) -> u32;
}

impl<T: CbObserverMut> CbObserver for T {
    fn get_read(&self) -> u32 {
        unsafe {
            (self as *const _ as *mut T)
                .as_mut()
                .unwrap_unchecked()
                .get_read_mut()
        }
    }

    fn get_write(&self) -> u32 {
        unsafe {
            (self as *const _ as *mut T)
                .as_mut()
                .unwrap_unchecked()
                .get_write_mut()
        }
    }

    fn get_capacity(&self) -> u32 {
        unsafe {
            (self as *const _ as *mut T)
                .as_mut()
                .unwrap_unchecked()
                .get_capacity_mut()
        }
    }
}

pub trait CbProducerMut {
    fn set_write_mut(&mut self, value: u32);
    fn set_data_mut(&mut self, offset: u32, value: &[u8]);
}

impl<T: CbProducerMut + CbObserver> CbProducer for T {
    fn set_write(&self, value: u32) {
        unsafe {
            (self as *const _ as *mut T)
                .as_mut()
                .unwrap_unchecked()
                .set_write_mut(value)
        }
    }

    fn set_data(&self, offset: u32, value: &[u8]) {
        unsafe {
            (self as *const _ as *mut T)
                .as_mut()
                .unwrap_unchecked()
                .set_data_mut(offset, value)
        }
    }
}

pub trait CbConsumerMut {
    fn set_read_mut(&mut self, value: u32);
    fn get_data_mut(&mut self, offset: u32, data: &mut [u8]);
}

impl<T: CbConsumerMut + CbObserver> CbConsumer for T {
    fn set_read(&self, value: u32) {
        unsafe {
            (self as *const _ as *mut T)
                .as_mut()
                .unwrap_unchecked()
                .set_read_mut(value)
        }
    }

    fn get_data(&self, offset: u32, value: &mut [u8]) {
        unsafe {
            (self as *const _ as *mut T)
                .as_mut()
                .unwrap_unchecked()
                .get_data_mut(offset, value)
        }
    }
}
