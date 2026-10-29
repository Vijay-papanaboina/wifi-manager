use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=resources/icons.gresource.xml");
    println!("cargo:rerun-if-changed=svgs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let target = out_dir.join("icons.gresource");
    let status = Command::new("glib-compile-resources")
        .arg("--sourcedir=.")
        .arg("--target")
        .arg(target)
        .arg("resources/icons.gresource.xml")
        .status()
        .expect("failed to run glib-compile-resources");
    assert!(status.success(), "glib-compile-resources failed with {status}");
}
