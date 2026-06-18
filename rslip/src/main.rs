use std::path::PathBuf;

fn main() {
    let repo = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("current directory"));
    match rslip::refresh_line_coverage_and_store(&repo, pyfork::default_parallelism()) {
        Ok(db) => {
            println!(
                "rslip refreshed {} file records and {} test records",
                db.files.len(),
                db.tests.len()
            );
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}
