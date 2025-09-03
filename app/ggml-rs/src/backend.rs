use crate::{
    compute_graph, device,
    execution_manager::ExecutionManager,
    ggml_sys::{
        ggml_backend, ggml_backend_dev_t, ggml_backend_i, ggml_backend_reg, ggml_backend_reg_i,
        ggml_backend_reg_t, ggml_backend_t, ggml_cgraph, ggml_guid, ggml_status,
        ggml_status_GGML_STATUS_FAILED,
    },
};

#[unsafe(no_mangle)]
pub extern "C" fn get_device_count(_backend: ggml_backend_reg_t) -> usize {
    device::count()
}

#[unsafe(no_mangle)]
pub extern "C" fn get_device(_backend: ggml_backend_reg_t, index: usize) -> ggml_backend_dev_t {
    device::get(index).unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn get_proc_address(
    _backend: ggml_backend_reg_t,
    _name: *const std::ffi::c_char,
) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}

static BACKEND_REG_INTERFACE: ggml_backend_reg_i = ggml_backend_reg_i {
    get_name: Some(backend_reg_get_name),
    get_device_count: Some(get_device_count),
    get_device: Some(get_device),
    get_proc_address: Some(get_proc_address),
};

pub static mut BACKEND_REG: ggml_backend_reg = ggml_backend_reg {
    api_version: 1,
    iface: BACKEND_REG_INTERFACE,
    context: std::ptr::null_mut(),
};

static BACKEND_NAME: &str = "ttx-rs\0";
unsafe extern "C" fn backend_reg_get_name(_backend: ggml_backend_reg_t) -> *const i8 {
    BACKEND_NAME.as_ptr() as *const i8
}
unsafe extern "C" fn backend_get_name(_backend: ggml_backend_t) -> *const i8 {
    BACKEND_NAME.as_ptr() as *const i8
}

unsafe extern "C" fn free(_backend: ggml_backend_t) {}

unsafe extern "C" fn graph_compute(
    backend: ggml_backend_t,
    graph: *mut ggml_cgraph,
) -> ggml_status {
    unsafe {
        if let (Some(_backend), Some(chip), Some(graph)) =
            (backend.as_mut(), get_device_from(backend), graph.as_mut())
        {
            compute_graph::compute(chip, graph)
        } else {
            ggml_status_GGML_STATUS_FAILED
        }
    }
}

pub static mut GUID: ggml_guid = [
    0xa9, 0x56, 0x35, 0x18, 0xab, 0x80, 0x4c, 0xf9, 0xb1, 0xff, 0xe9, 0x9c, 0x14, 0x7d, 0x3b, 0xdb,
];
pub static mut BACKEND: ggml_backend = ggml_backend {
    guid: std::ptr::null_mut(),
    iface: ggml_backend_i {
        get_name: Some(backend_get_name),
        free: Some(free),
        set_tensor_async: None,
        get_tensor_async: None,
        cpy_tensor_async: None,
        synchronize: None,
        graph_plan_create: None,
        graph_plan_free: None,
        graph_plan_update: None,
        graph_plan_compute: None,
        graph_compute: Some(graph_compute),
        event_record: None,
        event_wait: None,
    },
    device: std::ptr::null_mut(),
    context: std::ptr::null_mut(),
};

pub unsafe fn backend_init(dev: ggml_backend_dev_t) -> ggml_backend_t {
    unsafe {
        BACKEND.guid = &raw mut GUID;
        BACKEND.device = dev;
        &raw mut BACKEND
    }
}

pub unsafe fn get_device_from(backend: *mut ggml_backend) -> Option<&'static mut ExecutionManager> {
    unsafe {
        backend
            .as_mut()
            .and_then(|v| device::get_device_from(v.device))
    }
}
