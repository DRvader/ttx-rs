use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    for dir in std::fs::read_dir("target_def").unwrap().flatten() {
        if dir.file_type().unwrap().is_file() {
            let filename = dir.file_name().to_str().unwrap().to_string();
            let path = dir.path();

            println!("cargo:rerun-if-changed={}", path.display());

            // Put the linker script somewhere the linker can find it
            fs::File::create(out_dir.join(filename))
                .unwrap()
                .write_all(&std::fs::read(path).unwrap())
                .unwrap();
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
}
