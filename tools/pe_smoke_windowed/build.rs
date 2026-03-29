fn main() {
    println!("cargo:rustc-link-arg=/ENTRY:mainCRTStartup");
    println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
}
