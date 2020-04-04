use hyper::service::{make_service_fn, service_fn};
use hyper::{body::Buf, Body, Request, Response, Server};
use std::{convert::Infallible, net::SocketAddr};
use stream_body::{StreamBody, StreamData};
use tokio::fs::File;

async fn handle(_: Request<Body>) -> Result<Response<StreamBody<File>>, Infallible> {
    // Ok(Response::new("Hello, World!".into()))

    let file = File::open("./Cargo.toml").await.unwrap();

    let sb = StreamBody::new(file);

    Ok(Response::new(sb))
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
