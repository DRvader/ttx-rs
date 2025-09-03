use std::env;
use std::path::PathBuf;

fn main() {
    println!("{:?}", std::env::current_dir());
    let path = env::var("GGML_PATH").unwrap_or("./ggml/".to_string());
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    let bindings = bindgen::Builder::default()
        .headers([
            format!("{path}/include/ggml.h"),
            format!("{path}/src/ggml-impl.h"),
            format!("{path}/include/ggml-backend.h"),
            format!("{path}/src/ggml-backend-impl.h"),
        ])
        .clang_arg(format!("-I{path}/include"))
        .allowlist_recursively(true)
        .allowlist_function("ggml_backend_cpu_buffer_type")
        .allowlist_function("ggml_backend_cpu_buffer_from_ptr")
        .allowlist_function("ggml_backend_buft_is_host")
        .allowlist_function("ggml_nelements")
        .allowlist_function("ggml_type_size")
        .allowlist_type("ggml_backend")
        .allowlist_type("ggml_backend_reg_t")
        .allowlist_type("ggml_backend_dev_t")
        .rustified_enum("ggml_op")
        .rustified_enum("ggml_type")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");
    bindings
        .write_to_file(out_path.join("ggml_backend.rs"))
        .expect("Couldn't write bindings!");
}
