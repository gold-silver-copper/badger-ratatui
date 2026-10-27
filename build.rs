use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or(std::io::ErrorKind::NotFound)?);
    fs::write(out.join("memory.x"), include_bytes!("memory.x"))?;
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    Ok(())
}
