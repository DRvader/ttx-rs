use std::path::PathBuf;

pub fn write_cargo_toml(enable_defmt: bool) -> String {
    let path = env!("CARGO_MANIFEST_DIR").parse::<PathBuf>().unwrap();
    let shared_path = path.parent().unwrap().join("runtime-shared");
    let relocate_path = path.parent().unwrap().parent().unwrap().join("relocate");

    format!(
        r#"
        [package]
        name = "firmware"
        version = "0.1.0"
        edition = "2024"

        # HACK(drosen): This gets around errors due to this package being in a workspace
        [workspace]

        [dependencies]
        tensix-std = {{path = "{}/../../../tensix-std"}}
        runtime-shared = {{path = "{}"}}
        postcard = "1.1.1"
        {defmt}
        relocate = {{path = "{}"}}
        "#,
        env!("CARGO_MANIFEST_DIR"),
        shared_path.display(),
        relocate_path.display(),
        defmt = if enable_defmt {
            "defmt = \"1.0.0\""
        } else {
            ""
        }
    )
}

pub fn write_main(global: &str, brisc: &str) -> String {
    format!(
        r#"
    #![no_std]
    #![no_main]

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

    #[unsafe(no_mangle)]
    static JOB_LAUNCHED: SYNC<u32> = SYNC::new(0);

    #[unsafe(no_mangle)]
    static CORE_ID: SYNC<i32> = SYNC::new(-1);

    type SharedCoreLaunchData = Option<(u32, runtime_shared::CoreLaunchData)>;
    static NCRISC_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
    static NCRISC_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);
    static TRISC0_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
    static TRISC0_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);
    static TRISC1_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
    static TRISC1_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);
    static TRISC2_JOB_POINTER: SyncUnsafeCell<SharedCoreLaunchData> = SyncUnsafeCell::new(None);
    static TRISC2_JOB_RESULT: SyncUnsafeCell<Option<()>> = SyncUnsafeCell::new(None);

    use tensix_std::entry;

    fn dyn_base() -> u32 {{
        unsafe extern "Rust" {{
            unsafe static mut __firmware_end: u8;
        }}

        core::ptr::addr_of!(__firmware_end) as u32
    }}

    {global}

    fn jump_to_stack(addr: u32, new_sp: u32) {{
        unsafe {{
            let mut old_sp: usize;

            core::arch::asm!(
                "mv {{0}}, sp",
                out(reg) old_sp,
            );

            let old_sp_ref = &mut old_sp;

            core::arch::asm!(
                "mv sp, {{0}}",
                in(reg) new_sp,
            );

            core::mem::transmute::<u32, fn()>(addr)();

            let old_sp = *old_sp_ref;

            core::arch::asm!(
                "mv {{0}}, sp",
                in(reg) old_sp,
            );
        }}
    }}

    fn jump_to(base_addr: u32, addr: u32, new_sp: Option<u32>) {{
        if let Some(new_sp) = new_sp {{
            jump_to_stack(base_addr + addr, new_sp)
        }} else {{
            unsafe {{
                core::mem::transmute::<u32, fn()>(base_addr + addr)()
            }}
        }}
    }}

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {{
        unsafe {{
            let mut i = 0;
            while i < n {{
                dest.add(i).write(src.add(i).read());
                i += 1;
            }}
            dest
        }}
    }}

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn memset(dest: *mut u8, src: core::ffi::c_int, n: usize) -> *mut u8 {{
        unsafe {{
            let mut i = 0;
            while i < n {{
                dest.add(i).write(src as u8);
                i += 1;
            }}
            dest
        }}
    }}

    #[entry(brisc)]
    unsafe fn brisc_main() -> ! {{
        tensix_std::reset::start_cores();

        unsafe {{
            JOB_LAUNCHED.write(4);

            while CORE_ID.read() == -1 {{}}

            loop {{
                {brisc}

                NCRISC_JOB_RESULT.write(None);
                TRISC0_JOB_RESULT.write(None);
                TRISC1_JOB_RESULT.write(None);
                TRISC2_JOB_RESULT.write(None);

                // Zero BSS
                for addr in job.bss..job.ebss {{
                    *((base_addr + addr) as *mut u32) = 0;
                }}

                JOB_LAUNCHED.write(200);

                NCRISC_JOB_POINTER.write(Some((base_addr, job.ncrisc)));
                TRISC0_JOB_POINTER.write(Some((base_addr, job.trisc0)));
                TRISC1_JOB_POINTER.write(Some((base_addr, job.trisc1)));
                TRISC2_JOB_POINTER.write(Some((base_addr, job.trisc2)));

                JOB_LAUNCHED.write(201);

                jump_to(base_addr, job.brisc.entry, job.brisc.stack);

                JOB_LAUNCHED.write(202);

                loop {{
                    if NCRISC_JOB_RESULT.read().is_none() {{
                        JOB_LAUNCHED.write(203);
                        continue;
                    }}

                    if TRISC0_JOB_RESULT.read().is_none() {{
                        JOB_LAUNCHED.write(204);
                        continue;
                    }}

                    if TRISC1_JOB_RESULT.read().is_none() {{
                        JOB_LAUNCHED.write(205);
                        continue;
                    }}

                    if TRISC2_JOB_RESULT.read().is_none() {{
                        JOB_LAUNCHED.write(206);
                        continue;
                    }}

                    break;
                }}

                JOB_LAUNCHED.write(202);
            }}
        }}
    }}

    #[entry(ncrisc)]
    unsafe fn ncrisc_main() -> ! {{
        loop {{
            if let Some((base, info)) = NCRISC_JOB_POINTER.read() {{
                NCRISC_JOB_POINTER.write(None);
                jump_to(base, info.entry, info.stack);
                NCRISC_JOB_RESULT.write(Some(()));
            }}
        }}
    }}

    #[entry(trisc0)]
    unsafe fn trisc0_main() -> ! {{
        loop {{
            if let Some((base, info)) = TRISC0_JOB_POINTER.read() {{
                TRISC0_JOB_POINTER.write(None);
                jump_to(base, info.entry, info.stack);
                TRISC0_JOB_RESULT.write(Some(()));
            }}
        }}
    }}

    #[entry(trisc1)]
    unsafe fn trisc1_main() -> ! {{
        loop {{
            if let Some((base, info)) = TRISC1_JOB_POINTER.read() {{
                TRISC1_JOB_POINTER.write(None);
                jump_to(base, info.entry, info.stack);
                TRISC1_JOB_RESULT.write(Some(()));
            }}
        }}
    }}

    #[entry(trisc2)]
    unsafe fn trisc2_main() -> ! {{
        loop {{
            if let Some((base, info)) = TRISC2_JOB_POINTER.read() {{
                TRISC2_JOB_POINTER.write(None);
                jump_to(base, info.entry, info.stack);
                TRISC2_JOB_RESULT.write(Some(()));
            }}
        }}
    }}
    "#,
    )
}
