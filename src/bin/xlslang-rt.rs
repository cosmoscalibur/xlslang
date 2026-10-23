//! Slim runtime stub. `xlslang build` appends a compiled program to a copy of
//! this binary (or of the full `xlslang`) to produce a standalone executable.

fn main() {
    let args = xlslang::cli::Args(std::env::args().skip(1).collect());
    let code = match xlslang::bundle::read_embedded() {
        Some(program) => {
            xlslang::cli::with_big_stack(move || xlslang::cli::run_program(&program, args))
        }
        None => {
            eprintln!(
                "xlslang-rt: no program embedded; this is the runtime stub used by `xlslang build`"
            );
            2
        }
    };
    std::process::exit(code);
}
