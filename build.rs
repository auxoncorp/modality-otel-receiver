fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .compile(
            // files to compile
            &["opentelemetry-proto/opentelemetry/proto/collector/trace/v1/trace_service.proto"],
            // include path
            &["opentelemetry-proto"],
        )?;
    Ok(())
}
