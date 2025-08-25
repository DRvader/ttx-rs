use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone)]
pub enum StandardTarget {
    Grayskull,
    Wormhole,
    Blackhole,
}

impl Display for StandardTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            StandardTarget::Grayskull => "Grayskull",
            StandardTarget::Wormhole => "Wormhole",
            StandardTarget::Blackhole => "Blackhole",
        };
        f.write_str(name)
    }
}

impl StandardTarget {
    pub fn to_string(&self) -> String {
        match self {
            StandardTarget::Grayskull => "grayskull",
            StandardTarget::Wormhole => "wormhole",
            StandardTarget::Blackhole => "blackhole",
        }
        .to_string()
    }
}

#[derive(Clone)]
pub enum Rewrite {
    Replace {
        start: String,
        end: String,
        replace: String,
    },
    Add {
        value: String,
    },
}

#[derive(Clone)]
pub enum StandardTargetOrCustom {
    Standard((StandardTarget, Vec<Rewrite>)),
    Custom(String),
}

#[derive(Clone)]
pub enum TensixTarget {
    Standard(StandardTarget),
    Custom {
        name: String,
        target_def: StandardTargetOrCustom,
        linker_script: String,
    },
}

impl Display for TensixTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TensixTarget::Standard(s) => s.fmt(f),
            TensixTarget::Custom { name, .. } => f.write_str(name.as_str()),
        }
    }
}

impl TensixTarget {
    pub fn to_string(&self) -> String {
        match self {
            Self::Standard(s) => s.to_string(),
            TensixTarget::Custom { name, .. } => name.clone(),
        }
    }
}

pub enum CargoProfile {
    Release,
    Debug,
    Other(String),
}

impl CargoProfile {
    pub fn to_string(&self) -> String {
        match self {
            CargoProfile::Release => "release".to_string(),
            CargoProfile::Debug => "dev".to_string(),
            CargoProfile::Other(value) => value.to_string(),
        }
    }
}

#[derive(Clone)]
pub enum CacheEnable {
    CustomDir(PathBuf),
    Enabled,
    Disabled,
}

pub struct CargoOptions {
    pub target: TensixTarget,
    pub profile: CargoProfile,
    pub lto: bool,
    pub use_cache: CacheEnable,
    pub verbose: bool,
    pub build_std: bool,
    pub default_features: bool,
    pub stack_probes: bool,
    pub kernel_name: String,
    pub hide_output: bool,
    pub extra_flags: Vec<String>,
}

// Check if we might be running inside a cargo invocation.
// Will assume that this is true if we can invoke `cargo metadata`
// If we just append /tensix-builder to it to avoid a deadlock
pub fn get_target_dir() -> Option<PathBuf> {
    if let Ok(metadata) = cargo_metadata::MetadataCommand::new().exec() {
        Some(metadata.target_directory.as_std_path().to_path_buf())
    } else {
        None
    }
}

fn get_compiler_artifact(stdout: impl AsRef<str>) -> Option<CargoResult> {
    for message in cargo_metadata::Message::parse_stream(stdout.as_ref().as_bytes()) {
        let message = message.unwrap();
        match message {
            cargo_metadata::Message::CompilerArtifact(artifact) => {
                if let Some(artifact) = artifact.executable {
                    return Some(CargoResult {
                        path: artifact.as_std_path().to_path_buf(),
                        bin: true,
                    });
                }
            }
            _ => {}
        }
    }

    // No executable found... maybe search for a staticlib?
    for message in cargo_metadata::Message::parse_stream(stdout.as_ref().as_bytes()) {
        let message = message.unwrap();
        match message {
            cargo_metadata::Message::CompilerArtifact(artifact) => {
                if artifact.target.kind.contains(&"staticlib".to_string())
                    || artifact.target.kind.contains(&"cdylib".to_string())
                {
                    if let Some(filename) = artifact.filenames.get(0) {
                        return Some(CargoResult {
                            path: filename.as_std_path().to_path_buf(),
                            bin: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    None
}

pub struct CargoResult {
    pub path: PathBuf,
    pub bin: bool,
}

pub fn invoke_cargo<P: AsRef<Path>>(path: P, options: CargoOptions) -> CargoResult {
    let target_map = HashMap::from([
        (
            "grayskull",
            include_bytes!("../target_def/grayskull.json").as_slice(),
        ),
        (
            "wormhole",
            include_bytes!("../target_def/wormhole.json").as_slice(),
        ),
        (
            "blackhole",
            include_bytes!("../target_def/blackhole.json").as_slice(),
        ),
    ]);

    let target = options.target.to_string();

    let target_dir = get_target_dir().unwrap_or_else(|| {
        <PathBuf as std::str::FromStr>::from_str("target")
            .expect("could not create path from static const")
    });
    let kernel_target_dir = target_dir.join(format!("tensix-builder/{}", options.kernel_name));
    let kernel_target_cache_dir =
        target_dir.join(format!("tensix-builder/cache/link/{}", options.kernel_name));
    std::fs::create_dir_all(&kernel_target_cache_dir).expect("to be able to create directory");

    let mut linker_path = None;

    let target_def_file = match options.target {
        TensixTarget::Standard(standard_target) => {
            let file = kernel_target_cache_dir.join(format!("{target}.json"));
            let contents = target_map[standard_target.to_string().as_str()];
            let mut overwrite = true;
            if let Ok(existing) = std::fs::read(&file) {
                if existing == contents {
                    overwrite = false;
                }
            }

            if overwrite {
                std::fs::write(&file, contents).unwrap();
            }

            file
        }
        TensixTarget::Custom {
            name,
            target_def,
            linker_script,
        } => {
            let file = match target_def {
                StandardTargetOrCustom::Standard((s, mut rewrites)) => {
                    let target_json = target_map[s.to_string().as_str()];
                    let mut target_json = String::from_utf8(target_json.to_vec()).unwrap();

                    // Always rewrite the link arg
                    rewrites.insert(
                        0,
                        Rewrite::Replace {
                            start: "\"pre-link-args\"".to_string(),
                            end: "},".to_string(),
                            replace: format!(
                                "\"pre-link-args\": {{ \"gnu-lld\": [\"-T{name}.x\"] "
                            ),
                        },
                    );

                    for rewrite in rewrites {
                        match rewrite {
                            Rewrite::Replace {
                                start,
                                end,
                                replace,
                            } => {
                                let start_pos = target_json.find(&start).unwrap();
                                let end_pos = target_json[start_pos..].find(&end).unwrap();

                                target_json = format!(
                                    "{}{}{}",
                                    &target_json[..start_pos],
                                    replace,
                                    &target_json[start_pos..][end_pos..]
                                );
                            }
                            Rewrite::Add { value } => {
                                let end_pos = target_json.rfind("\n}").unwrap();
                                target_json = format!(
                                    "{}{}{}",
                                    &target_json[..end_pos],
                                    value,
                                    &target_json[end_pos..]
                                );
                            }
                        }
                    }

                    // for (index, line) in target_json.lines().enumerate() {
                    // println!("{index}: {line}");
                    // }

                    let file = kernel_target_cache_dir.join(format!("{name}.json"));
                    let mut overwrite = true;
                    if let Ok(existing) = std::fs::read(&file) {
                        if existing == target_json.as_bytes() {
                            overwrite = false;
                        }
                    }
                    if overwrite {
                        std::fs::write(&file, target_json).unwrap();
                    }

                    file
                }
                StandardTargetOrCustom::Custom(c) => {
                    let file = kernel_target_cache_dir.join(format!("{name}.json"));
                    let mut overwrite = true;
                    if let Ok(existing) = std::fs::read(&file) {
                        if existing == c.as_bytes() {
                            overwrite = false;
                        }
                    }
                    if overwrite {
                        std::fs::write(&file, c).unwrap();
                    }

                    file
                }
            };

            let link_file = kernel_target_cache_dir.join(format!("{name}.x"));
            let mut overwrite = true;
            if let Ok(existing) = std::fs::read(&link_file) {
                if existing == linker_script.as_bytes() {
                    overwrite = false;
                }
            }
            if overwrite {
                std::fs::write(&link_file, linker_script).unwrap();
            }

            linker_path = Some(kernel_target_cache_dir);

            file
        }
    };

    let build_std = if options.build_std {
        "-Zbuild-std"
    } else {
        "-Zbuild-std=core,alloc"
    };

    let mut cargo = Command::new("cargo");
    cargo.args([
        "+nightly",
        "build",
        "--message-format=json-render-diagnostics",
        "--target",
        &target_def_file.to_string_lossy(),
        "--profile",
        &options.profile.to_string(),
        build_std,
    ]);

    if options.verbose {
        cargo.arg("--verbose");
    }

    if !options.default_features {
        cargo.arg("--no-default-features");
    }

    cargo.args(["--target-dir", &kernel_target_dir.to_string_lossy()]);

    let mut kernel_name = options.kernel_name;
    if !kernel_name.starts_with('"') || !kernel_name.ends_with('"') {
        kernel_name = format!("\"{kernel_name}\"");
    }
    let mut flags = format!("--cfg kernel_name={kernel_name}");
    if let Some(linker_path) = linker_path {
        flags = format!("{flags} -L {}", linker_path.display());
    }

    for flag in options.extra_flags.into_iter() {
        flags.push(' ');
        flags.push_str(&flag);
    }

    cargo.env("RUSTFLAGS", flags);

    if let CacheEnable::Enabled | CacheEnable::CustomDir(_) = options.use_cache {
        cargo.env("RUSTC_WRAPPER", "sccache");
    }
    if let CacheEnable::CustomDir(dir) = options.use_cache {
        cargo.env("SCCACHE_DIR", format!("{}", dir.display()));
    }

    if options.lto {
        cargo.env(
            format!("CARGO_PROFILE_{}_LTO", options.profile.to_string()),
            "true",
        );
    }

    if options.stack_probes {
        unimplemented!("Don't know how to configure")
    }

    let mut build = cargo.current_dir(&path);
    if options.hide_output {
        build = build.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        build = build.stdout(Stdio::piped()).stderr(Stdio::inherit());
    }

    let build = build.output().expect("Failed to execute cargo build");

    if build.status.success() {
        get_compiler_artifact(String::from_utf8(build.stdout).unwrap()).unwrap_or_else(|| {
            if options.hide_output {
                eprintln!(
                    "--- build output ---\n{}",
                    String::from_utf8(build.stderr).unwrap()
                );
            }
            panic!("build artifact not found in (supposedly successful) build output (see above)");
        })
    } else {
        if options.hide_output {
            eprintln!(
                "--- build output ---\n{}",
                String::from_utf8(build.stderr).unwrap()
            );
        }
        panic!("Cargo build did not complete successfully (see above)");
    }
}
