use std::io::Result;

fn main() -> Result<()> {
    // Compile Protocol Buffers definitions to Rust code
    prost_build::Config::new()
        .out_dir("src/proto")
        .compile_protos(&["proto/metrics.proto"], &["proto/"])?;

    // Tell Cargo to rerun build script if proto files change
    println!("cargo:rerun-if-changed=proto/metrics.proto");

    Ok(())
}
