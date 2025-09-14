pub use chip::{open, Chip};
pub use luwen_core::Arch;

pub use macros::kernel;
pub use tensix_builder;

pub mod chip;
pub mod kernel;
pub mod loader;

pub fn enumerate() -> Vec<usize> {
    ttkmd_if::PciDevice::scan()
}

#[macro_export]
macro_rules! load {
    ($name:expr, $device:ident, $core:expr) => {
        $crate::load!($name, $device, $core,)
    };
    (
        $name:expr, $device:ident, $core:expr, $( $key:ident = $value:expr ),* $(,)?
    ) => {{
        let base_path = ::std::path::PathBuf::from(file!())
            .parent()
            .unwrap()
            .to_path_buf();
        let options = $crate::loader::LoadOptions::new(base_path.as_path())$(.$key($value))*;

        let mut _device = $device.dupe().unwrap();
        $crate::loader::quick_load(
            stringify!($name),
            _device,
            $core,
            options,
        )
    }};
}
