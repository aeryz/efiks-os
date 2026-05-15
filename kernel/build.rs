fn main() {
    println!("cargo:rerun-if-changed=src/linker.ld");
    println!("cargo:rustc-link-arg-bin=kernel=-Tkernel/src/linker.ld");
}
