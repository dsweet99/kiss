use std::path::PathBuf;

fn main() {
    let repo = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("current directory"));
    let collector = rslip::PytestTraceCollector;
    match rslip::refresh_and_store(&repo, &|root, selectors, _j| {
        collector.collect(root, selectors, _j)
    }, pyfork::default_parallelism()) {
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
