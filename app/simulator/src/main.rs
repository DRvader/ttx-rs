use std::collections::HashMap;

enum Cell {
    // A positive edge D-type flip-flop with positive polarity enable.
    DffePp {
        c: i64,
        d: i64,
        e: i64,
        q: i64,
    },
    Lut2 {
        a: i64,
        b: i64,
        y: i64,
        lut: [bool; 4],
    },
}

pub struct LutRemoteOutput {
    noc: bool,
    x: u8,
    y: u8,
    address: u32,
}

pub struct Lut2 {
    input_0: u32,
    input_1: u32,
    state: [bool; 4],
}

pub struct Lut2Local {
    data: Lut2,
    local: Vec<u32>,
}

pub struct Lut2Remote {
    data: Lut2,
    remote: Vec<LutRemoteOutput>,
    local: Vec<u32>,
}

pub struct RemoteCore {
    instructions: Vec<Lut2Remote>,
}

pub struct Core {
    instructions: Vec<Lut2Local>,
}

pub struct Lut2Builder {
    state: [bool; 4],
    remote: Vec<TensixLocation>,
    local: Vec<TensixLocation>,
}

pub struct Tensix {
    id: usize,
    memory: usize,
    luts: Vec<Lut2Builder>,
}

impl Tensix {
    fn new_group(count: usize, l1: usize) -> Vec<Tensix> {
        let mut output = Vec::new();
        for i in 0..count {
            output.push(Tensix {
                id: i,
                memory: l1,
                luts: Vec::new(),
            });
        }

        output
    }

    fn place(&mut self, state: [bool; 4]) -> bool {
        if self.memory > 4 {
            self.luts.push(Lut2Builder {
                state,
                remote: Vec::new(),
                local: Vec::new(),
            });
            self.memory -= 4;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TensixLocation {
    tensix_id: usize,
    lut_id: usize,
    offset: bool,
}

pub fn connect(tensix: &mut [Tensix], from: TensixLocation, to: TensixLocation) {
    if from.tensix_id == to.tensix_id {
        tensix[from.tensix_id].luts[from.lut_id].local.push(to);
    } else {
        tensix[from.tensix_id].luts[from.lut_id].remote.push(to);
    }
}

impl Tensix {}

fn main() {
    let json_file = std::fs::read("design.json").unwrap();
    let value: serde_json::Value = serde_json::from_slice(&json_file).unwrap();

    let mut mapping = Vec::new();

    let top = value.as_object().unwrap();
    for (name, module) in top["modules"].as_object().unwrap() {
        println!("module: {name}");
        println!("ports");
        for port in module["ports"].as_object().unwrap() {
            println!("{}", port.0);
        }
        for (cell_name, values) in module["cells"].as_object().unwrap() {
            let values = values.as_object().unwrap();
            let connections = values["connections"].as_object().unwrap();
            let parameters = values["parameters"].as_object().unwrap();

            match values["type"].as_str().unwrap() {
                "$_DFFE_PP_" => {
                    mapping.push(Cell::DffePp {
                        c: connections["C"].as_array().unwrap()[0].as_i64().unwrap(),
                        d: connections["D"].as_array().unwrap()[0].as_i64().unwrap(),
                        e: connections["E"].as_array().unwrap()[0].as_i64().unwrap(),
                        q: connections["Q"].as_array().unwrap()[0].as_i64().unwrap(),
                    });
                }
                "$lut" => {
                    let mut lut = parameters["LUT"].as_str().unwrap().chars();
                    let a = connections["A"].as_array().unwrap();
                    let y = connections["Y"].as_array().unwrap();
                    assert_eq!(a.len(), 2);
                    mapping.push(Cell::Lut2 {
                        a: a[0].as_i64().unwrap(),
                        b: a[1].as_i64().unwrap(),
                        y: y[0].as_i64().unwrap(),
                        lut: [
                            lut.next().unwrap() != '0',
                            lut.next().unwrap() != '0',
                            lut.next().unwrap() != '0',
                            lut.next().unwrap() != '0',
                        ],
                    });
                }
                ty => {
                    unimplemented!("No support for cell of type {ty}");
                }
            }
        }
    }

    let mut location_lookup = HashMap::new();
    let mut tensix = Tensix::new_group(100, 500 * 1024);

    let mut not_full_tensix = 0;

    for cell in mapping {
        match cell {
            Cell::DffePp { c, d, e, q } => todo!(),
            Cell::Lut2 { a, b, y, lut } => {
                while !tensix[not_full_tensix].place(lut) {
                    not_full_tensix += 1;
                }

                let output = TensixLocation {
                    tensix_id: not_full_tensix,
                    lut_id: tensix[not_full_tensix].luts.len() - 1,
                    offset: false,
                };

                location_lookup.insert(y, output).unwrap();

                let a = location_lookup.get(&a).unwrap();
                let b = location_lookup.get(&b).unwrap();

                connect(&mut tensix, output, TensixLocation { offset: true, ..*a });
                connect(&mut tensix, output, *b);
            }
        }
    }

    let mut global = String::new();
    let mut preamble = String::new();
    let mut postamble = String::new();

    global.push_str(
        r#"
        fn compute(old_result: bool, a: u32, b: u32, state: [bool; 4]) -> bool {{
            let a = (a as *mut u32).read_volatile();
            let b = (b as *mut u32).read_volatile();

            let result = state[((b << 1) | a) as usize]
            old_result != result
        }}

        fn noc_write(x: u8, y: u8, offset: u32, value: bool) {{
            unsafe {{
                tensix_std::target::noc::noc_write(
                    tensix_std::target::noc::NocCommandSel::default(),
                    tensix_std::target::noc::NocAddr {{
                        offset: offset,
                        x_end: x,
                        y_end: y,
                        ..Default::default()
                    }},
                    &value.to_le_bytes(),
                    true
                );
            }}
        }}
    "#,
    );

    let chip = ttx_rs::open(0).unwrap();

    for t in tensix {
        let mut fw = String::new();

        for (id, lut) in t.luts.into_iter().enumerate() {
            preamble.push_str(&format!("let mut old_result_{id} = false;\n"));

            let mut push_result = String::new();

            for remote in lut.remote {
                let tile = chip.tensix(remote.tensix_id);
                push_result.push_str(&format!(
                    "noc_write({}, {}, {}, !old_result_{})\n",
                    tile.get(ttx_rs::chip::noc::NocId::Noc0).0,
                    tile.get(ttx_rs::chip::noc::NocId::Noc0).1,
                    remote.lut_id + remote.offset as u8 as usize,
                    id
                ));
            }

            for local in lut.local {
                push_result.push_str(&format!("{} = !old_result_{}\n", local.lut_id, id));
            }

            // postamble.push_str(&format!(
            //     r#"
            //     {{
            //         let changed = compute(old_result_{id}, {}, {}, {state:?});
            //         old_result_{id} ^= changed;
            //         if changed {{
            //             {}
            //         }}
            //     }}
            //    "#,
            //     lut.a, lut.b, lut.state, push_result
            // ));
        }
    }
}
