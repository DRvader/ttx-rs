#[inline]
fn right_shift(existing: &mut [u8], shift: u32) {
    let byte_shift = shift as usize / 8;
    let bit_shift = shift as usize % 8;

    if shift as usize >= existing.len() * 8 {
        for o in existing {
            *o = 0;
        }
        return;
    }

    if byte_shift > 0 {
        for index in 0..existing.len() {
            existing[index] = *existing.get(index + byte_shift).unwrap_or(&0);
        }
    }

    if bit_shift > 0 {
        let mut carry = 0;
        for i in (0..existing.len()).rev() {
            let next_carry = (existing[i] & ((1 << bit_shift) - 1)) << (8 - bit_shift);
            existing[i] = (existing[i] >> bit_shift) | carry;
            carry = next_carry;
        }
    }
}

#[allow(dead_code)]
#[inline]
fn left_shift(existing: &mut [u8], shift: u32) {
    let byte_shift = shift as usize / 8;
    let bit_shift = shift as usize % 8;

    if shift as usize >= existing.len() * 8 {
        for o in existing {
            *o = 0;
        }
        return;
    }

    if byte_shift > 0 {
        for index in (0..existing.len()).rev() {
            let shifted = if index < byte_shift {
                0
            } else {
                existing[index - byte_shift]
            };
            existing[index] = shifted;
        }
    }

    if bit_shift > 0 {
        let mut carry = 0;
        for i in (0..existing.len()).rev() {
            let next_carry = (existing[i] & ((1 << bit_shift) - 1) << (8 - bit_shift)) >> bit_shift;
            existing[i] = (existing[i] << bit_shift) | carry;
            carry = next_carry;
        }
    }
}

#[inline]
fn mask_off(existing: &mut [u8], high_bit: u32) -> &mut [u8] {
    let top_byte = high_bit as usize / 8;
    let top_bit = high_bit % 8;

    if top_byte < existing.len() {
        existing[top_byte] &= (1 << top_bit) - 1;
    }

    let len = existing.len();
    &mut existing[0..(top_byte + 1).min(len)]
}

/// Take a value and place it onto the existing value shifting by, `lower` and masking off at `upper`
fn write_modify(existing: &mut [u8], value: &[u8], lower: u32, upper: u32) {
    assert!(upper >= lower);
    assert!(existing.len() * 8 > upper as usize);

    let mut shift_count = upper - lower + 1;
    let mut read_ptr = 0;
    let mut write_ptr = lower / 8;
    let write_shift = lower % 8;

    let mut first_time_lengthen = write_shift;

    let mut carry = existing[write_ptr as usize] & ((1 << write_shift) - 1);
    while shift_count > 0 {
        let to_write = (value.get(read_ptr as usize).copied().unwrap_or(0) << write_shift) | carry;
        if write_shift > 0 {
            carry = (value.get(read_ptr as usize).copied().unwrap_or(0) >> (8 - write_shift))
                & ((1 << write_shift) - 1);
        }

        let write_count = shift_count.min(8 - first_time_lengthen) as u16;
        let write_mask = ((1 << (write_count + first_time_lengthen as u16).min(8)) - 1) as u8;

        first_time_lengthen = 0;

        existing[write_ptr as usize] =
            (to_write & write_mask) | (existing[write_ptr as usize] & !write_mask);

        read_ptr += 1;
        write_ptr += 1;

        shift_count -= write_count as u32;
    }
}

fn read_modify(existing: &mut [u8], lower: u32, upper: u32) -> &[u8] {
    assert!(upper >= lower);
    assert!(existing.len() * 8 > upper as usize);

    right_shift(existing, lower);
    &*mask_off(existing, upper - lower + 1)
}

#[derive(Copy, Clone)]
pub struct Field {
    /// Byte Address of base of field
    pub addr: u64,
    /// Size of field in bytes (how far to read past the base)
    pub size: usize,
    /// The bits coverered by the field (lower, upper)
    pub bits: Option<(u32, u32)>,
}

pub fn read_field<'a, C>(
    context: &mut C,
    mut read_func: impl FnMut(&mut C, u64, &mut [u8]),
    field: Field,
    value: &'a mut [u8],
) -> Option<&'a [u8]> {
    if value.len() < field.size {
        // return Err(AxiError::ReadBufferTooSmall)?;
        return None;
    }

    read_func(context, field.addr, &mut value[..field.size]);

    let value = if let Some((lower, upper)) = field.bits {
        read_modify(value, lower, upper);

        value
    } else {
        &mut value[..field.size]
    };

    Some(&*value)
}

pub fn write_field<C>(
    context: &mut C,
    mut read_func: impl FnMut(&mut C, u64, &mut [u8]),
    mut write_func: impl FnMut(&mut C, u64, &[u8]),
    field: Field,
    existing: &mut [u8],
    value: &[u8],
) -> Option<()> {
    if value.len() < field.size {
        // return Err(AxiError::ReadBufferTooSmall)?;
        return None;
    }

    if let Some((lower, upper)) = field.bits {
        read_func(context, field.addr, existing);

        write_modify(existing, value, lower, upper);

        write_func(context, field.addr, existing);
    } else {
        // We are writing the full size of the field
        write_func(context, field.addr, &value[..field.size]);
    };

    Some(())
}

pub fn read_field_u32<C>(
    context: &mut C,
    read_func: impl FnMut(&mut C, u64, &mut [u8]),
    field: Field,
) -> u32 {
    let mut output = [0; 4];
    read_field(context, read_func, field, &mut output);

    u32::from_le_bytes(output)
}

pub fn write_field_u32<C>(
    context: &mut C,
    read_func: impl FnMut(&mut C, u64, &mut [u8]),
    write_func: impl FnMut(&mut C, u64, &[u8]),
    field: Field,
    value: u32,
) {
    let mut temp = [0; 4];
    write_field(
        context,
        read_func,
        write_func,
        field,
        &mut temp,
        &value.to_le_bytes(),
    );
}

pub fn write_field_vec<C>(
    context: &mut C,
    read_func: impl FnMut(&mut C, u64, &mut [u8]),
    write_func: impl FnMut(&mut C, u64, &[u8]),
    field: Field,
    value: &[u8],
) {
    let mut temp = vec![0; field.size];
    write_field(context, read_func, write_func, field, &mut temp, value);
}
