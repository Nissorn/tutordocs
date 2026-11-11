fn main() {
    std::process::exit(tutordocs_lib::ctl::run_cli(
        std::env::args().skip(1).collect(),
    ));
}
