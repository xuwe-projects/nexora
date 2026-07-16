#[cfg(feature = "zitadel")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    generate("zitadel/user/v2/user_service.proto")?;
    generate("zitadel/project/v2/project_service.proto")?;
    Ok(())
}

#[cfg(not(feature = "zitadel"))]
fn main() {}

#[cfg(feature = "zitadel")]
fn generate(input: &str) -> Result<(), std::io::Error> {
    grpc_protobuf_build::CodeGen::new()
        .include("proto")
        .input(input)
        .client_only()
        .compile()
        .map_err(std::io::Error::other)
}
