use std::io::{self, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut err = io::stderr();
    match decimal::cli::run(&args, &mut io::stdin().lock(), &mut out, &mut err) {
        Ok(status) => ExitCode::from(status as u8),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("decimal: {e}");
            ExitCode::FAILURE
        }
    }
}
