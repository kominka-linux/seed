fn main() {
    seed::install_signal_handlers();
    let code = seed::dispatch(&seed::argv());
    std::process::exit(code);
}
