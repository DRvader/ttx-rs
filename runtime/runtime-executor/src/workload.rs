use std::collections::HashMap;

use relocate::KernelRelocation;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use ttx_rs::{Arch, kernel::KernelData, loader::LoadOptions, tensix_builder::Rewrite};

use super::firmware::build_kernel_elf_cached;

#[derive(Default, Hash, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputBuffer {
    pub name: String,
    pub size: usize,
    pub count: usize,
}

pub struct OutputSlot {
    pub name: String,
    pub size: usize,
    pub index: usize,
}

impl OutputSlot {
    pub fn symbol_base(&self) -> String {
        format!("_BUFFER_{}", self.name)
    }

    fn slot_symbol_base(&self) -> String {
        format!("{}_SLOT_{}", self.symbol_base(), self.index)
    }

    pub fn symbol_read(&self) -> String {
        format!("{}_READ_INDEX", self.slot_symbol_base())
    }

    pub fn symbol_write(&self) -> String {
        format!("{}_WRITE_INDEX", self.symbol_base())
    }

    pub fn symbol_flushed(&self) -> String {
        format!("{}_FLUSHED", self.symbol_base())
    }

    pub fn symbol_data(&self) -> String {
        format!("{}_DATA", self.symbol_base())
    }
}

impl OutputBuffer {
    pub fn output_count(&self) -> String {
        format!("_COMPLETION_COUNT_{}", self.name)
    }

    pub fn output_buffer(&self) -> String {
        self.name.clone()
    }

    pub fn get_slot(&self, index: usize) -> Option<OutputSlot> {
        if index >= self.count {
            None
        } else {
            Some(OutputSlot {
                name: self.name.clone(),
                size: self.size,
                index,
            })
        }
    }
}

#[derive(Default, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadBuilder {
    pub use_local_cache: bool,

    pub available_space: u64,

    pub inputs: Vec<String>,
    pub outputs: Vec<OutputBuffer>,

    pub global: String,
    pub brisc: String,
    pub ncrisc: String,
    pub trisc0: String,
    pub trisc1: String,
    pub trisc2: String,
}

impl WorkloadBuilder {
    pub fn input_buffer(&mut self, name: impl AsRef<str>, size: usize) {
        let name = name.as_ref();

        self.global.push_str(&format!(
            r#"
            #[unsafe(no_mangle)]
            static {name}: NocAlignment<u8, {size}> = NocAlignment::new(0);
          "#
        ));
        self.inputs.push(name.to_string());
    }

    pub fn output_buffer(
        &mut self,
        name: impl AsRef<str>,
        size: usize,
        // The number of nodes waiting on the completion
        completion_slots: usize,
    ) -> OutputBuffer {
        // To add empty full detection we need to burn a slot
        let size = size + 1;
        let name = name.as_ref();

        let buffer_name = format!("_BUFFER_{name}");

        let mut smallest_read = String::new();
        smallest_read.push_str(&format!(
            "fn smallest_read_for_{buffer_name}(write: u32) -> u32 {{"
        ));
        if completion_slots == 0 {
            smallest_read.push_str("let max_read = 0;\n");
        }

        for i in 0..completion_slots {
            let name = format!("{buffer_name}_SLOT_{i}_READ_INDEX");
            self.global.push_str(&format!(
                r#"
                #[unsafe(no_mangle)]
                static {name}: SYNC<u32> = SYNC::new(0);
            "#
            ));

            if i == 0 {
                smallest_read.push_str(&format!(
                    r#"
                    let mut max_read = {name}.read();
                    let mut max_count = buffer_count(write, max_read, {size});
                    "#
                ));
            } else {
                smallest_read.push_str(&format!(
                    r#"
                let new_max_read = {name}.read();
                let new_max_count = buffer_count(write, new_max_read, {size});
                if new_max_count > max_count {{
                    max_read = new_max_read;
                    max_count = new_max_count;
                }}
                "#
                ));
            }
        }

        smallest_read.push_str("max_read\n}");

        self.global.push_str(&smallest_read);

        {
            let name = format!("{buffer_name}_FLUSHED");
            self.global.push_str(&format!(
                r#"
            #[unsafe(no_mangle)]
            static {name}: SYNC<u32> = SYNC::new(0);
          "#
            ));
        }

        {
            let name = format!("{buffer_name}_WRITE_INDEX");
            self.global.push_str(&format!(
                r#"
            #[unsafe(no_mangle)]
            static {name}: SYNC<u32> = SYNC::new(0);
          "#
            ));
        }

        {
            let name = format!("{buffer_name}_DATA");
            self.global.push_str(&format!(
                r#"
            #[unsafe(no_mangle)]
            static {name}: SYNC<[u8; {size}]> = SYNC::new([0; {size}]);
          "#
            ));
        }

        OutputBuffer {
            name: name.to_string(),
            size: size - 1,
            count: completion_slots,
        }
    }

    pub fn iterate_zip(
        &mut self,
        shape: &[i64],
        strides: &[&[usize]],
        func: impl FnOnce(&[String]) -> String,
    ) -> String {
        let mut loops = Vec::new();
        let mut it = shape.iter().copied().enumerate();

        {
            if let Some((index, dim)) = it.next() {
                loops.push(format!("for _{index}_it in 0..{dim}u32 {{"));
                for (tensor_index, stride) in strides.iter().enumerate() {
                    loops.push(format!(
                        "let _{tensor_index}_tensor_{index}_dim_index = _{index}_it * {stride};",
                        stride = stride[index]
                    ));
                }
            }
        }

        for (index, dim) in it {
            loops.push(format!("for _{index}_it in 0..{dim}u32 {{"));
            for (tensor_index, stride) in strides.iter().enumerate() {
                loops.push(format!("let _{tensor_index}_tensor_{index}_dim_index = _{tensor_index}_tensor_{last_index}_dim_index + _{index}_it * {stride};", last_index = index - 1, stride = stride[index]));
            }
        }

        if !loops.is_empty() {
            let mut final_indexes = Vec::new();
            for tensor_index in 0..strides.len() {
                loops.push(format!(
                    "let _{tensor_index}_tensor_index = _{tensor_index}_tensor_{}_dim_index;",
                    shape.len().saturating_sub(1)
                ));
                final_indexes.push(format!("_{tensor_index}_tensor_index"));
            }
            loops.push(func(&final_indexes));
        }

        for _ in 0..shape.len() {
            loops.push("}".to_string());
        }

        loops.join("\n")
    }

    pub fn compile(self, arch: Arch) -> Workload {
        assert!(
            self.available_space > 0,
            "Cannot compile a workload that takes on 0 bytes"
        );

        Workload::compile(arch, self)
    }
}

pub struct Workload {
    pub path: Option<TempDir>,
    pub data: KernelData,
}

pub struct LoadedWorkload {
    pub workload: Workload,
    pub tile: ttx_rs::chip::noc::Tile,
    pub base_addr: u64,
}

impl LoadedWorkload {
    pub fn data(&self, index: impl AsRef<str>) -> u64 {
        self.base_addr + self.workload.data.sym_table[index.as_ref()]
    }
}

impl Workload {
    pub fn dupe(&self) -> Self {
        Self {
            path: None,
            data: self.data.clone(),
        }
    }
}

impl Workload {
    fn write_cargo_toml() -> String {
        format!(
            r#"
        [package]
        name = "kernel"
        version = "0.1.0"
        edition = "2024"

        # HACK(drosen): This gets around errors due to this package being in a workspace
        [workspace]

        [lib]
        crate-type = ["cdylib"]

        [dependencies]
        tensix-std = {{path = "{}/../../../tensix-std"}}
        "#,
            env!("CARGO_MANIFEST_DIR"),
        )
    }

    fn write_lib(files: &mut HashMap<String, String>, builder: WorkloadBuilder) {
        files.insert(
            "src/lib.rs".to_string(),
            format!(
                r#"
        #![no_std]
        #![no_main]

        #[repr(align(64))]
        struct NocAlignment<T, const N: usize>(pub [T; N]);

        impl<T, const N: usize> core::ops::Index<usize> for NocAlignment<T, N> {{
            type Output = T;

            fn index(&self, index: usize) -> &Self::Output {{
                &self.0[index]
            }}
        }}

        impl<T, const N: usize> core::ops::IndexMut<usize> for NocAlignment<T, N> {{
            fn index_mut(&mut self, index: usize) -> &mut Self::Output {{
                &mut self.0[index]
            }}
        }}

        impl<T, const N: usize> core::ops::Index<u32> for NocAlignment<T, N> {{
            type Output = T;

            fn index(&self, index: u32) -> &Self::Output {{
                &self.0[index as usize]
            }}
        }}

        impl<T, const N: usize> core::ops::IndexMut<u32> for NocAlignment<T, N> {{
            fn index_mut(&mut self, index: u32) -> &mut Self::Output {{
                &mut self.0[index as usize]
            }}
        }}

        impl<T: Copy, const N: usize> NocAlignment<T, N> {{
            pub const fn new(value: T) -> Self {{
                NocAlignment([value; N])
            }}
        }}

        impl<T, const N: usize> NocAlignment<T, N> {{
            pub fn addr(&self) -> u32 {{
                self.0.as_ptr() as u32
            }}

            pub fn len(&self) -> u32 {{
                N as u32
            }}

            pub fn read(&self, index: usize) -> T
                where T: Sized
            {{
                unsafe {{
                    self.0.as_ptr().add(index).read_volatile()
                }}
            }}

            pub fn write(&mut self, index: usize, value: T) {{
                unsafe {{
                    self.0.as_mut_ptr().add(index).write_volatile(value);
                }}
            }}
        }}

        #[repr(align(64))]
        pub struct NocAligned<T>(T);

        impl<T> core::ops::Deref for NocAligned<T> {{
            type Target = T;

            fn deref(&self) -> &<Self as core::ops::Deref>::Target {{
                &self.0
            }}
        }}

        impl<T> core::ops::DerefMut for NocAligned<T> {{
            fn deref_mut(&mut self) -> &mut <Self as core::ops::Deref>::Target {{
                &mut self.0
            }}
        }}

        #[repr(transparent)]
        pub struct SyncUnsafeCell<T>(core::cell::UnsafeCell<T>);
        unsafe impl<T: Sync> Sync for SyncUnsafeCell<T> {{}}

        impl<T> SyncUnsafeCell<T> {{
            pub const fn new(value: T) -> Self {{
                SyncUnsafeCell(core::cell::UnsafeCell::new(value))
            }}

            pub fn get(&self) -> *mut T {{
                self.0.get()
            }}

            pub fn read(&self) -> T {{
                unsafe {{
                    self.get().read_volatile()
                }}
            }}

            pub fn write(&self, value: T) {{
                unsafe {{
                    self.get().write_volatile(value);
                }}
            }}
        }}

        #[repr(transparent)]
        pub struct SyncUnsafeNocCell<T>(NocAligned<SyncUnsafeCell<T>>);

        impl<T> SyncUnsafeNocCell<T> {{
            pub const fn new(value: T) -> Self {{
                SyncUnsafeNocCell(NocAligned(SyncUnsafeCell(core::cell::UnsafeCell::new(value))))
            }}

            pub fn get(&self) -> *mut T {{
                ((self.0).0).0.get()
            }}

            pub fn read(&self) -> T {{
                unsafe {{
                    self.get().read_volatile()
                }}
            }}

            pub fn write(&self, value: T) {{
                unsafe {{
                    self.get().write_volatile(value);
                }}
            }}
        }}

        type SYNC<T> = SyncUnsafeNocCell<T>;

        struct Tile {{
            n0: (u8, u8),
            n1: (u8, u8)
        }}

        impl From<u32> for Tile {{
            fn from(value: u32) -> Self {{
                Tile {{
                    n0: (value as u8, (value >> 8) as u8),
                    n1: ((value >> 16) as u8, (value >> 24) as u8)
                }}
            }}
        }}

        fn buffer_count(write: u32, read: u32, size: u32) -> u32 {{
            if write >= read {{
                write - read
            }} else {{
                size - read + write
            }}
        }}

        fn buffer_pull(remote: Tile, data: u32, read: u32, write: u32, size: u32, value: &mut [u8]) {{
        }}

        fn buffer_push_dyn(smallest_read: fn(u32) -> u32, data: *mut u8, size: u32, write: &SYNC<u32>, value: &[u8]) {{
            unsafe {{
                for v in value {{
                    let write_value = write.read();

                    // Wait while buffer is full
                    while smallest_read(write_value) == (write_value + 1) % size {{ }}

                    data.add(write_value as usize).write_volatile(*v);

                    write.write((write_value + 1) % size);
                }}
            }}
        }}

        fn buffer_push<const SIZE: usize>(smallest_read: fn(u32) -> u32, data: &SYNC<[u8; SIZE]>, write: &SYNC<u32>, value: &[u8]) {{
            unsafe {{
                buffer_push_dyn(smallest_read, &raw mut ((*data.get())[0]), SIZE as u32, write, value)
            }}
        }}

        fn buffer_complete(smallest_read: fn(u32) -> u32, write: &SYNC<u32>, flushed: &SYNC<u32>) {{
            unsafe {{
                flushed.write(true as u8 as u32);

                let write = write.read();

                // Wait for buffer to be empty
                while write != smallest_read(write) {{}}
            }}
        }}

        #[panic_handler]
        fn panic(_info: &core::panic::PanicInfo) -> ! {{
            loop {{}}
        }}

        {global}

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn brisc_kmain() {{
            unsafe {{
                {brisc}
            }}
        }}

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn ncrisc_kmain() {{
            unsafe {{
                {ncrisc}
            }}
        }}

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn trisc0_kmain() {{
            unsafe {{
                {trisc0}
            }}
        }}

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn trisc1_kmain() {{
            unsafe {{
                {trisc1}
            }}
        }}

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn trisc2_kmain() {{
            unsafe {{
                {trisc2}
            }}
        }}
        "#,
                global = builder.global,
                brisc = builder.brisc,
                ncrisc = builder.ncrisc,
                trisc0 = builder.trisc0,
                trisc1 = builder.trisc1,
                trisc2 = builder.trisc2,
            ),
        );
    }

    pub fn compile(arch: Arch, builder: WorkloadBuilder) -> Self {
        let available_space_for_workload = builder.available_space;

        let mut files = HashMap::new();

        files.insert("Cargo.toml".to_string(), Self::write_cargo_toml());
        Self::write_lib(&mut files, builder.clone());

        let link_script = match arch {
            Arch::Grayskull => include_str!("workload_link/grayskull-kernel.x"),
            Arch::Wormhole => include_str!("workload_link/wormhole-kernel.x"),
            Arch::Blackhole => include_str!("workload_link/blackhole-kernel.x"),
            Arch::Unknown(_) => todo!(),
        };
        let link_script = link_script.replace(
            "{available_space}",
            &available_space_for_workload.to_string(),
        );

        let (path, (kernel_data, _elf)) = build_kernel_elf_cached(
            arch,
            builder,
            LoadOptions::new_without_base(),
            Some((
                link_script,
                vec![
                    Rewrite::Replace {
                        start: "\"relocation-model\"".to_string(),
                        end: ",".to_string(),
                        replace: "\"relocation-model\": \"pic\"".to_string(),
                    },
                    Rewrite::Add {
                        value: ",\n\"dynamic-linking\": true\n".to_string(),
                    },
                ],
            )),
            files,
        );

        Workload {
            path,
            data: kernel_data,
        }
    }

    pub fn get_binary(&self) -> (Box<[KernelRelocation]>, Box<[u8]>) {
        let mut kernel_binary = Vec::new();
        for write in &self.data.writes {
            // u32
            kernel_binary.extend_from_slice(&write.addr.to_le_bytes());
            if write.addr as usize + write.len() > kernel_binary.len() {
                kernel_binary.resize(write.addr as usize + write.len(), 0);
            }
            kernel_binary[write.addr as usize..][..write.len()].copy_from_slice(&write.data.0);
        }

        (
            self.data.relocations.clone().into_boxed_slice(),
            kernel_binary.into_boxed_slice(),
        )
    }
}
