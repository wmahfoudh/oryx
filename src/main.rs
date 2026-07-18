fn main() {
    let mut args = std::env::args().skip(1);
    if let Some("--version") = args.next().as_deref() {
        println!("oryx {}", env!("CARGO_PKG_VERSION"));
    }
}
