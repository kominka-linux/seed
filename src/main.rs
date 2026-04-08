fn main() {
    seed::common::runtime::install_signal_handlers();
    let code = seed::dispatch(&seed::common::runtime::argv());
    std::process::exit(code);
}
