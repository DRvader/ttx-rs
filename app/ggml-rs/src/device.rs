use std::sync::OnceLock;

use crate::{
    backend,
    execution_manager::ExecutionManager,
    ggml_sys::{
        ggml_backend_buffer_t, ggml_backend_buffer_type_t, ggml_backend_cpu_buffer_type,
        ggml_backend_dev_caps, ggml_backend_dev_props, ggml_backend_dev_t, ggml_backend_dev_type,
        ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_ACCEL, ggml_backend_device,
        ggml_backend_device_i, ggml_backend_t, ggml_tensor,
    },
};

static DEVICES: OnceLock<Vec<GgmlDevice>> = OnceLock::new();

pub struct GgmlDevice {
    chip: Box<ExecutionManager>,
    device: Box<ggml_backend_device>,
}

unsafe impl Send for GgmlDevice {}
unsafe impl Sync for GgmlDevice {}

unsafe extern "C" fn get_name(device: *mut ggml_backend_device) -> *const i8 {
    unsafe {
        if let Some(device) = get_device_from(device) {
            Box::leak(format!("{}\0", device.chip).into_bytes().into_boxed_slice()).as_ptr()
                as *const i8
        } else {
            std::ptr::null()
        }
    }
}

static DESCRIPTION: &str = "ttx-rs\0";
unsafe extern "C" fn get_description(_device: *mut ggml_backend_device) -> *const i8 {
    DESCRIPTION.as_ptr() as *const i8
}

unsafe extern "C" fn get_memory(
    _device: *mut ggml_backend_device,
    free: *mut usize,
    total: *mut usize,
) {
    unsafe {
        free.write(0);
        total.write(0);
    }
}

unsafe extern "C" fn get_type(_device: *mut ggml_backend_device) -> ggml_backend_dev_type {
    ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_ACCEL
}

unsafe extern "C" fn get_props(
    device: *mut ggml_backend_device,
    props: *mut ggml_backend_dev_props,
) {
    unsafe {
        if let Some(props) = props.as_mut() {
            props.name = get_name(device);
            props.description = get_description(device);
            props.type_ = get_type(device);
            props.caps = ggml_backend_dev_caps {
                async_: false,
                host_buffer: false,
                buffer_from_host_ptr: false,
                events: false,
            };
            props.memory_free = 0;
            props.memory_total = 0;
        }
    }
}

unsafe extern "C" fn init_backend(dev: ggml_backend_dev_t, _params: *const i8) -> ggml_backend_t {
    unsafe { backend::backend_init(dev) }
}

unsafe extern "C" fn get_buffer_type(_dev: ggml_backend_dev_t) -> ggml_backend_buffer_type_t {
    unsafe { ggml_backend_cpu_buffer_type() }
}

unsafe extern "C" fn buffer_from_host_buffer(
    _dev: ggml_backend_dev_t,
    buffer: *mut std::ffi::c_void,
    size: usize,
    _max_tensor_size: usize,
) -> ggml_backend_buffer_t {
    unsafe { crate::ggml_sys::ggml_backend_cpu_buffer_from_ptr(buffer, size) }
}

unsafe extern "C" fn supports_op(dev: ggml_backend_dev_t, op: *const ggml_tensor) -> bool {
    unsafe {
        if let (Some(dev), Some(chip), Some(op)) = (dev.as_mut(), get_device_from(dev), op.as_ref())
        {
            crate::compute_graph::supports_op(dev, chip, op)
        } else {
            false
        }
    }
}

unsafe extern "C" fn supports_buft(
    _dev: ggml_backend_dev_t,
    buft: ggml_backend_buffer_type_t,
) -> bool {
    unsafe { crate::ggml_sys::ggml_backend_buft_is_host(buft) }
}

pub fn get_all() -> &'static [GgmlDevice] {
    DEVICES
        .get_or_init(|| {
            ttx_rs::chip::scan()
                .into_iter()
                .filter_map(|v| v.ok())
                .map(|v| {
                    let chip = Box::new(ExecutionManager::new(v));
                    let chip_addr = &raw const *chip;
                    GgmlDevice {
                        device: Box::new(ggml_backend_device {
                            iface: ggml_backend_device_i {
                                get_name: Some(get_name),
                                get_description: Some(get_description),
                                get_memory: Some(get_memory),
                                get_type: Some(get_type),
                                get_props: Some(get_props),
                                init_backend: Some(init_backend),
                                get_buffer_type: Some(get_buffer_type),
                                get_host_buffer_type: None,
                                buffer_from_host_ptr: Some(buffer_from_host_buffer),
                                supports_op: Some(supports_op),
                                supports_buft: Some(supports_buft),
                                offload_op: None,
                                event_new: None,
                                event_free: None,
                                event_synchronize: None,
                            },
                            reg: &raw mut crate::backend::BACKEND_REG,
                            context: chip_addr as *mut std::ffi::c_void,
                        }),
                        chip,
                    }
                })
                .collect()
        })
        .as_slice()
}

pub fn count() -> usize {
    get_all().len()
}

pub fn get(index: usize) -> Option<*mut ggml_backend_device> {
    get_all()
        .get(index)
        .map(|v| &raw const *v.device)
        .map(|v| v as *mut _)
}

pub unsafe fn get_device_from(
    dev: *mut ggml_backend_device,
) -> Option<&'static mut ExecutionManager> {
    unsafe {
        dev.as_mut()
            .and_then(|v| (v.context as *mut ExecutionManager).as_mut())
    }
}
