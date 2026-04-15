fn main() {
    seed::common::runtime::install_signal_handlers();
    let argv = seed::common::runtime::argv();
    let code = seed::wget::main(&argv[1..]);
    std::process::exit(code);
}
