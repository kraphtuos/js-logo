//! Print a code as SVG: `cargo run -p ringcode --example svg -- JSC > jsc.svg`

fn main() {
    let word = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: svg <WORD>");
        std::process::exit(2);
    });
    match ringcode::svg(&word, &ringcode::SvgOptions::default()) {
        Ok(svg) => println!("{svg}"),
        Err(e) => {
            eprintln!("{word}: {e}");
            std::process::exit(1);
        }
    }
}
