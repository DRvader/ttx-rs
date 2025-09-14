use runtime_executor::workload::WorkloadBuilder;
use ttx_rs::Chip;

use crate::{
    execution_manager::ExecutionManager,
    ggml_sys::{
        ggml_backend_device, ggml_cgraph, ggml_status, ggml_status_GGML_STATUS_SUCCESS,
        ggml_tensor, ggml_type, ggml_type_size,
    },
};

pub struct Shape {}

pub struct GgmlTypeTraits(ggml_type);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TtxTypes {
    F64,
    F32,
    I8,
    I16,
    I32,
    I64,
}

impl TtxTypes {
    pub fn to_ggml(&self) -> ggml_type {
        match self {
            TtxTypes::F64 => ggml_type::GGML_TYPE_F64,
            TtxTypes::F32 => ggml_type::GGML_TYPE_F32,
            TtxTypes::I8 => ggml_type::GGML_TYPE_I8,
            TtxTypes::I16 => ggml_type::GGML_TYPE_I16,
            TtxTypes::I32 => ggml_type::GGML_TYPE_I32,
            TtxTypes::I64 => ggml_type::GGML_TYPE_I64,
        }
    }

    pub fn compat(&self, other: TtxTypes) -> (Self, Self, Self) {
        match (self, other) {
            (TtxTypes::F64, _) | (_, TtxTypes::F64) => (TtxTypes::F64, TtxTypes::F64, TtxTypes::F64),
            (TtxTypes::F32, _) | (_, TtxTypes::F32) => (TtxTypes::F32, TtxTypes::F32, TtxTypes::F32),
            (TtxTypes::I64, _) | (_, TtxTypes::I64) => (TtxTypes::I64, TtxTypes::I64, TtxTypes::I64),
            (TtxTypes::I32, _) | (_, TtxTypes::I32) => (TtxTypes::I32, TtxTypes::I32, TtxTypes::I32),
            (TtxTypes::I16, _) | (_, TtxTypes::I16) => (TtxTypes::I16, TtxTypes::I16, TtxTypes::I16),
            (TtxTypes::I8, _) /* | (_, TtxTypes::I8) */ => (TtxTypes::I8, TtxTypes::I8, TtxTypes::I8),
        }
    }

    pub fn is_float_like(&self) -> bool {
        match self {
            TtxTypes::F64 | TtxTypes::F32 => true,
            TtxTypes::I8 | TtxTypes::I16 | TtxTypes::I32 | TtxTypes::I64 => false,
        }
    }

    pub fn is_int_like(&self) -> bool {
        match self {
            TtxTypes::F64 | TtxTypes::F32 => false,
            TtxTypes::I8 | TtxTypes::I16 | TtxTypes::I32 | TtxTypes::I64 => true,
        }
    }

    pub fn gen_from(&self, input: ggml_type) -> Option<String> {
        let conversion_line = match (input, self) {
            /*
            (ggml_type::GGML_TYPE_F16, TtxTypes::F32) => {
                r#"
                let w = input << 16;
                let sign = w & 0x80000000;
                let two_w = w + w;

                let exp_offset = 0xE0 << 23;

                let exp_scale = core::f32::from_bits(0x7800000);
                let normalized_value = core::f32::from_bits((two_w >> 4) + exp_offset) * exp_scale;

                let magic_mask = 126 << 23;
                let magic_bias = 0.5;
                let denormalized_value = core::f32::from_bits((two_w >> 17) | magic_mask) - magic_bias;

                let denormalized_cutoff = 1 << 27;
                let result = sign | if two_w < denormalized_cutoff { core::f32::from_bits(denormalized_value) } else { core::f32::from_bits(normalized_value) };

                core::f32::from_bits(result)
                "#
            }
            */
            (ggml_type::GGML_TYPE_F32, TtxTypes::F32) => "input",
            (ggml_type::GGML_TYPE_I8, TtxTypes::I8) => "input",
            (ggml_type::GGML_TYPE_I16, TtxTypes::I16) => "input",
            (ggml_type::GGML_TYPE_I32, TtxTypes::I32) => "input",
            (ggml_type::GGML_TYPE_I64, TtxTypes::I64) => "input",
            (ggml_type::GGML_TYPE_F64, TtxTypes::F64) => "input",
            (input, output) => {
                tracing::warn!("Have not yet implemented support for {input:?} => {output:?}");
                return None;
            }
        };

        Some(format!(
            r#"
                (|input| {{
                    {conversion_line}
                }})
            "#
        ))
    }

    pub fn gen_to(&self, output: ggml_type) -> Option<String> {
        let conversion_line = match (self, output) {
            /*
            (TtxTypes::F32, ggml_type::GGML_TYPE_F16) => {
                r#"
                let scale_to_inf = core::f32::from_bits(0x77800000);
                let scale_to_zero = core::f32::from_bits(0x08800000);

                let base = input.abs() * scale_to_inf * scale_to_zero;

                let w = f.to_bits();
                let shl1_w = w + w;
                let sign = w & 0x80000000;
                let bias = (shl1_w & 0xFF000000).min(0x71000000);

                let base = core::f32::from_bits((bias >> 1) + 0x07800000) + base;
                let bits = core::f32::to_bits(base);
                let exp_bits = (bits >> 13) & 0x00007C00;
                let mantissa_bits = bits & 0x00000FFF;
                let nonsign = exp_bits + mantissa_bits;
                core::f32::from_bits((sign >> 16) | if shl1_w > 0xFF000000 { 0x7E00 }  else { nonsign })
                "#
            }
            */
            (TtxTypes::F32, ggml_type::GGML_TYPE_F32) => "input",
            (TtxTypes::I8, ggml_type::GGML_TYPE_I8) => "input",
            (TtxTypes::I16, ggml_type::GGML_TYPE_I16) => "input",
            (TtxTypes::I32, ggml_type::GGML_TYPE_I32) => "input",
            (TtxTypes::I64, ggml_type::GGML_TYPE_I64) => "input",
            (TtxTypes::F64, ggml_type::GGML_TYPE_F64) => "input",
            (input, output) => {
                tracing::warn!("Have not yet implemented support for {input:?} => {output:?}");
                return None;
            }
        };

        Some(format!(
            r#"
            (|input| {{
                {conversion_line}
            }})
        "#
        ))
    }

    pub fn gen_convert(&self, output: TtxTypes) -> Option<String> {
        let output = if self == &output {
            "input".to_string()
        } else {
            let name = match output {
                TtxTypes::F64 => "f64",
                TtxTypes::F32 => "f32",
                TtxTypes::I8 => "i8",
                TtxTypes::I16 => "i16",
                TtxTypes::I32 => "i32",
                TtxTypes::I64 => "i64",
            };

            format!("input as {name}")
        };

        Some(format!("(|input| {output})"))
    }
}

impl GgmlTypeTraits {
    pub fn is_float_like(&self) -> bool {
        match self.0 {
            ggml_type::GGML_TYPE_F32
            | ggml_type::GGML_TYPE_F16
            | ggml_type::GGML_TYPE_Q4_0
            | ggml_type::GGML_TYPE_Q4_1
            | ggml_type::GGML_TYPE_Q5_0
            | ggml_type::GGML_TYPE_Q5_1
            | ggml_type::GGML_TYPE_Q8_0
            | ggml_type::GGML_TYPE_Q8_1
            | ggml_type::GGML_TYPE_Q2_K
            | ggml_type::GGML_TYPE_Q3_K
            | ggml_type::GGML_TYPE_Q4_K
            | ggml_type::GGML_TYPE_Q5_K
            | ggml_type::GGML_TYPE_Q6_K
            | ggml_type::GGML_TYPE_Q8_K
            | ggml_type::GGML_TYPE_IQ2_XXS
            | ggml_type::GGML_TYPE_IQ2_XS
            | ggml_type::GGML_TYPE_IQ3_XXS
            | ggml_type::GGML_TYPE_IQ1_S
            | ggml_type::GGML_TYPE_IQ4_NL
            | ggml_type::GGML_TYPE_IQ3_S
            | ggml_type::GGML_TYPE_IQ2_S
            | ggml_type::GGML_TYPE_IQ4_XS
            | ggml_type::GGML_TYPE_IQ1_M
            | ggml_type::GGML_TYPE_BF16
            | ggml_type::GGML_TYPE_TQ1_0
            | ggml_type::GGML_TYPE_TQ2_0
            | ggml_type::GGML_TYPE_MXFP4
            | ggml_type::GGML_TYPE_F64 => true,
            ggml_type::GGML_TYPE_I8
            | ggml_type::GGML_TYPE_I16
            | ggml_type::GGML_TYPE_I32
            | ggml_type::GGML_TYPE_I64
            | ggml_type::GGML_TYPE_COUNT => false,
        }
    }

    pub fn is_int_like(&self) -> bool {
        match self.0 {
            ggml_type::GGML_TYPE_F32
            | ggml_type::GGML_TYPE_F16
            | ggml_type::GGML_TYPE_Q4_0
            | ggml_type::GGML_TYPE_Q4_1
            | ggml_type::GGML_TYPE_Q5_0
            | ggml_type::GGML_TYPE_Q5_1
            | ggml_type::GGML_TYPE_Q8_0
            | ggml_type::GGML_TYPE_Q8_1
            | ggml_type::GGML_TYPE_Q2_K
            | ggml_type::GGML_TYPE_Q3_K
            | ggml_type::GGML_TYPE_Q4_K
            | ggml_type::GGML_TYPE_Q5_K
            | ggml_type::GGML_TYPE_Q6_K
            | ggml_type::GGML_TYPE_Q8_K
            | ggml_type::GGML_TYPE_IQ2_XXS
            | ggml_type::GGML_TYPE_IQ2_XS
            | ggml_type::GGML_TYPE_IQ3_XXS
            | ggml_type::GGML_TYPE_IQ1_S
            | ggml_type::GGML_TYPE_IQ4_NL
            | ggml_type::GGML_TYPE_IQ3_S
            | ggml_type::GGML_TYPE_IQ2_S
            | ggml_type::GGML_TYPE_IQ4_XS
            | ggml_type::GGML_TYPE_IQ1_M
            | ggml_type::GGML_TYPE_BF16
            | ggml_type::GGML_TYPE_TQ1_0
            | ggml_type::GGML_TYPE_TQ2_0
            | ggml_type::GGML_TYPE_MXFP4
            | ggml_type::GGML_TYPE_F64 => false,
            ggml_type::GGML_TYPE_I8
            | ggml_type::GGML_TYPE_I16
            | ggml_type::GGML_TYPE_I32
            | ggml_type::GGML_TYPE_I64 => true,
            ggml_type::GGML_TYPE_COUNT => false,
        }
    }

    pub fn natural_type(&self) -> Option<TtxTypes> {
        let ty = match self.0 {
            ggml_type::GGML_TYPE_F32
            | ggml_type::GGML_TYPE_F16
            | ggml_type::GGML_TYPE_Q4_0
            | ggml_type::GGML_TYPE_Q4_1
            | ggml_type::GGML_TYPE_Q5_0
            | ggml_type::GGML_TYPE_Q5_1
            | ggml_type::GGML_TYPE_Q8_0
            | ggml_type::GGML_TYPE_Q8_1
            | ggml_type::GGML_TYPE_Q2_K
            | ggml_type::GGML_TYPE_Q3_K
            | ggml_type::GGML_TYPE_Q4_K
            | ggml_type::GGML_TYPE_Q5_K
            | ggml_type::GGML_TYPE_Q6_K
            | ggml_type::GGML_TYPE_Q8_K
            | ggml_type::GGML_TYPE_IQ2_XXS
            | ggml_type::GGML_TYPE_IQ2_XS
            | ggml_type::GGML_TYPE_IQ3_XXS
            | ggml_type::GGML_TYPE_IQ1_S
            | ggml_type::GGML_TYPE_IQ4_NL
            | ggml_type::GGML_TYPE_IQ3_S
            | ggml_type::GGML_TYPE_IQ2_S
            | ggml_type::GGML_TYPE_IQ4_XS
            | ggml_type::GGML_TYPE_IQ1_M
            | ggml_type::GGML_TYPE_BF16
            | ggml_type::GGML_TYPE_MXFP4
            | ggml_type::GGML_TYPE_TQ1_0
            | ggml_type::GGML_TYPE_TQ2_0 => TtxTypes::F32,
            ggml_type::GGML_TYPE_F64 => TtxTypes::F64,
            ggml_type::GGML_TYPE_I8 => TtxTypes::I8,
            ggml_type::GGML_TYPE_I16 => TtxTypes::I16,
            ggml_type::GGML_TYPE_I32 => TtxTypes::I32,
            ggml_type::GGML_TYPE_I64 => TtxTypes::I64,
            ggml_type::GGML_TYPE_COUNT => return None,
        };

        Some(ty)
    }
}

fn broadcast_tensors<const N: usize>(tensors: [&ggml_tensor; N]) -> ([i64; 4], [[usize; 4]; N]) {
    let mut output_shape = [1; 4];
    for d in 0..4 {
        for t in tensors.iter() {
            output_shape[d] = output_shape[d].max(t.ne[d]);
        }
    }

    let mut broadcasted = [(); N].map(|_| [0; 4]); // dummy init
    for (i, t) in tensors.iter().enumerate() {
        let mut new_strides = t.nb;
        for d in 0..4 {
            if t.ne[d] == 1 && output_shape[d] > 1 {
                new_strides[d] = 0;
            } else if t.ne[d] != output_shape[d] {
                panic!(
                    "Incompatible shapes for broadcasting to {output_shape:?}\n{:?}\n failed at dim {}",
                    tensors, d
                );
            }
        }
        broadcasted[i] = new_strides;
    }

    (output_shape, broadcasted)
}

fn iterate(a: &ggml_tensor, mut func: impl FnMut(*mut std::ffi::c_void)) {
    for z in 0..a.ne[0] {
        let z_index = z * a.nb[0] as i64;
        for o in 0..a.ne[1] {
            let o_index = z_index + o * a.nb[1] as i64;
            for tw in 0..a.ne[2] {
                let tw_index = o_index + tw * a.nb[2] as i64;
                for th in 0..a.ne[3] {
                    let th_index = tw_index + th * a.nb[3] as i64;

                    unsafe { func(a.data.byte_add(th_index as usize)) };
                }
            }
        }
    }
}

fn iterate_zip<const N: usize>(
    tensors: [&ggml_tensor; N],
    mut func: impl FnMut([*mut std::ffi::c_void; N]),
) -> ([i64; 4], [[usize; 4]; N]) {
    let (shape, strides) = broadcast_tensors(tensors);

    for z in 0..shape[0] {
        let z_index = strides.map(|v| z * v[0] as i64);
        for o in 0..shape[1] {
            let o_index = {
                let mut index = 0;
                strides.map(|v| {
                    let output = z_index[index] + o * v[1] as i64;
                    index += 1;
                    output
                })
            };
            for tw in 0..shape[2] {
                let tw_index = {
                    let mut index = 0;
                    strides.map(|v| {
                        let output = o_index[index] + tw * v[2] as i64;
                        index += 1;
                        output
                    })
                };
                for th in 0..shape[3] {
                    let th_index = {
                        let mut index = 0;
                        strides.map(|v| {
                            let output = tw_index[index] + th * v[3] as i64;
                            index += 1;
                            output
                        })
                    };
                    let data = {
                        let mut index = 0;
                        th_index.map(|v| {
                            let output = unsafe { tensors[index].data.byte_add(v as usize) };
                            index += 1;
                            output
                        })
                    };

                    func(data);
                }
            }
        }
    }

    (shape, strides)
}

fn eltwise_split(
    _chip: &mut Chip,
    a: &mut ggml_tensor,
    b: &mut ggml_tensor,
    dest: &mut ggml_tensor,
) -> ([i64; 4], usize, Vec<(Vec<u8>, Vec<u8>)>) {
    // let size = elem_count * a.type + elem_count * b.type + elem_count * dest.type;
    // let size = elem_count * (a.type + b.type + dest.type);
    // let elem_count = size / (a.type + b.type + dest.type);

    let a_size = unsafe { ggml_type_size(a.type_) };
    let b_size = unsafe { ggml_type_size(b.type_) };
    let dest_size = unsafe { ggml_type_size(dest.type_) };

    let target_size = 1024;
    let elem_count = target_size as usize / (a_size + b_size + dest_size);

    let mut buffers = Vec::new();

    let mut a_buffer = vec![0; elem_count * a_size];
    let mut b_buffer = vec![0; elem_count * b_size];

    let mut running_count = 0;
    let (bcast_shape, _bcast_strides) = iterate_zip([a, b], |[a, b]| {
        for (index, o) in a_buffer[running_count * a_size..][..a_size]
            .iter_mut()
            .enumerate()
        {
            unsafe {
                *o = a.cast::<u8>().byte_add(index).read();
            }
        }

        for (index, o) in b_buffer[running_count * b_size..][..b_size]
            .iter_mut()
            .enumerate()
        {
            unsafe {
                *o = b.cast::<u8>().byte_add(index).read();
            }
        }

        running_count += 1;
        if running_count >= elem_count {
            buffers.push((
                std::mem::replace(&mut a_buffer, vec![0; elem_count * a_size]),
                std::mem::replace(&mut b_buffer, vec![0; elem_count * b_size]),
            ));
            running_count = 0;
        }
    });

    buffers.push((a_buffer, b_buffer));

    (bcast_shape, elem_count, buffers)
}

fn default_strides_for_shape(shape: [i64; 4], type_size: usize) -> [usize; 4] {
    let mut strides = [0; 4];

    let mut current_stride = type_size;
    for i in shape.as_slice().iter().copied().enumerate().rev() {
        strides[i.0] = current_stride;
        current_stride *= i.1 as usize;
    }

    strides
}

pub fn binary_op_calculate_output_type(
    a: ggml_type,
    b: ggml_type,
) -> Option<(TtxTypes, TtxTypes, TtxTypes)> {
    let a = GgmlTypeTraits(a);
    let b = GgmlTypeTraits(b);
    if let (Some(ttx_a), Some(ttx_b)) = (a.natural_type(), b.natural_type()) {
        Some(ttx_a.compat(ttx_b))
    } else {
        None
    }
}

pub fn add(
    manager: &mut ExecutionManager,
    a: &mut ggml_tensor,
    b: &mut ggml_tensor,
    dest: &mut ggml_tensor,
) {
    let (bcast_shape, elem_count, buffers) = eltwise_split(&mut manager.chip, a, b, dest);

    let mut kernel = WorkloadBuilder {
        available_space: manager.firmware.data.bin.data_start.unwrap_or(0),
        ..WorkloadBuilder::default()
    };

    kernel.input_buffer("a", elem_count * unsafe { ggml_type_size(a.type_) });
    kernel.input_buffer("b", elem_count * unsafe { ggml_type_size(b.type_) });
    let host_output = kernel.output_buffer(
        "dest",
        elem_count * unsafe { ggml_type_size(dest.type_) },
        1,
    );

    let strides = [
        default_strides_for_shape(bcast_shape, unsafe { ggml_type_size(a.type_) }),
        default_strides_for_shape(bcast_shape, unsafe { ggml_type_size(b.type_) }),
        default_strides_for_shape(bcast_shape, unsafe { ggml_type_size(dest.type_) }),
    ];
    let strides = [
        strides[0].as_slice(),
        strides[1].as_slice(),
        strides[2].as_slice(),
    ];

    let (inter_a, inter_b, standard_type) =
        binary_op_calculate_output_type(a.type_, b.type_).unwrap();

    kernel.brisc = kernel.iterate_zip(&bcast_shape, &strides, |tensors| {
        format!("dest[{dest_index}] = {convert_dest}({{ {convert_inter_a}({convert_a}(a[{a_index}])) }} + {{ {convert_inter_b}({convert_b}(b[{b_index}])) }})",
            a_index = &tensors[0], b_index= &tensors[1], dest_index = &tensors[2],
            convert_a = inter_a.gen_from(a.type_).unwrap(),
            convert_inter_a = inter_a.gen_convert(standard_type).unwrap(),
            convert_b = inter_b.gen_from(b.type_).unwrap(),
            convert_inter_b = inter_b.gen_convert(standard_type).unwrap(),
            convert_dest = standard_type.gen_to(dest.type_).unwrap(),
        )
    });

    let output = manager.queue(vec![(host_output.clone(), 0)], kernel, buffers);

    // let mut index = 0;
    // for o in output {
    //     for i in o {
    //         unsafe {
    //             *(dest.data as *mut u8).add(index) = i;
    //         }
    //         index += 1;
    //     }
    // }
}

pub fn add_type_check(
    _chip: &mut Chip,
    a: &mut ggml_tensor,
    b: &mut ggml_tensor,
    dest: &ggml_tensor,
) -> bool {
    if let Some((inter_a, inter_b, standard_type)) =
        binary_op_calculate_output_type(a.type_, b.type_)
    {
        inter_a.gen_from(a.type_).is_some()
            && inter_a.gen_convert(standard_type).is_some()
            && inter_b.gen_from(b.type_).is_some()
            && inter_b.gen_convert(standard_type).is_some()
            && standard_type.gen_to(dest.type_).is_some()
    } else {
        false
    }
}

pub unsafe fn supports_op(
    _device: &mut ggml_backend_device,
    manager: &mut ExecutionManager,
    op: &ggml_tensor,
) -> bool {
    unsafe {
        match op.op {
            crate::ggml_sys::ggml_op::GGML_OP_NONE => true,
            crate::ggml_sys::ggml_op::GGML_OP_ADD | crate::ggml_sys::ggml_op::GGML_OP_ADD1 => {
                let a = op.src[0].as_mut().unwrap();
                let b = op.src[1].as_mut().unwrap();

                add_type_check(&mut manager.chip, a, b, op)
            }
            _ => false,
        }
    }
}

pub unsafe fn compute(manager: &mut ExecutionManager, graph: &mut ggml_cgraph) -> ggml_status {
    let nodes = unsafe { std::slice::from_raw_parts_mut(graph.nodes, graph.n_nodes as usize) };
    unsafe {
        for node in nodes {
            if let Some(node) = node.as_mut() {
                match node.op {
                    crate::ggml_sys::ggml_op::GGML_OP_ADD
                    | crate::ggml_sys::ggml_op::GGML_OP_ADD1 => {
                        let a = node.src[0].as_mut().unwrap();
                        let b = node.src[1].as_mut().unwrap();

                        add(manager, a, b, node);
                    }
                    _ => (),
                }
            }
        }
    }

    ggml_status_GGML_STATUS_SUCCESS
}
