#[allow(dead_code)]
pub mod bundle {
    tonic::include_proto!("bundle");
}

#[allow(dead_code)]
pub mod packet {
    tonic::include_proto!("packet");
}

pub mod searcher {
    tonic::include_proto!("searcher");
}

#[allow(dead_code)]
pub mod shared {
    tonic::include_proto!("shared");
}
