fn main() {
    let protos = [
        "protos/shared.proto",
        "protos/packet.proto",
        "protos/bundle.proto",
        "protos/searcher.proto",
    ];

    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile(
            &[
                "protos/shared.proto",
                "protos/packet.proto",
                "protos/bundle.proto",
                "protos/searcher.proto",
            ],
            &["protos"],
        )
        .expect("compile Jito gRPC protos");
}
