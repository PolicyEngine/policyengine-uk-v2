fn main() {
    // The python extension cdylib must leave Python C-API symbols undefined
    // for the interpreter to resolve at import time (macOS needs explicit
    // `-undefined dynamic_lookup`). No-op for the rlib and the binary.
    pyo3_build_config::add_extension_module_link_args();
}
