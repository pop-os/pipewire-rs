// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::env;
use std::path::PathBuf;

fn main() {
    let libs = system_deps::Config::new()
        .probe()
        .expect("Cannot find libraries");

    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");

    let builder = bindgen::Builder::default()
        .header("wrapper.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        // Use `usize` for `size_t`. This behavior of bindgen changed because it is not
        // *technically* correct, but is the case in all architectures supported by Rust.
        .size_t_is_usize(true)
        .allowlist_function("spa_.*")
        .allowlist_type("spa_.*")
        .allowlist_var("spa_.*")
        .allowlist_var("SPA_.*")
        .prepend_enum_name(false)
        .derive_eq(true);

    let builder = libs
        .iter()
        .iter()
        .flat_map(|(_, lib)| lib.include_paths.iter())
        .fold(builder, |builder, l| {
            let arg = format!("-I{}", l.to_string_lossy());
            builder.clang_arg(arg)
        });

    let bindings = builder.generate().expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
