use ttx_rs::chip;

fn main() {
    for chip in chip::scan() {
        println!("{chip:?}");
    }
}
