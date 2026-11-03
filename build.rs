//! Link-time setup for the `oak-mcp` binary.
//!
//! `quercus-engine-rs` links the engine cdylib (`libquercus_engine.so`) and its
//! build script stages a copy next to the built binaries, but an rpath entry is
//! still required for the loader to find it without `LD_LIBRARY_PATH`.

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
	match target_os.as_str() {
		// Applies to bins, examples, and test harnesses alike.
		"linux" => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
		"macos" => println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path"),
		_ => {}
	}
}
