use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::{convert::Infallible, net::SocketAddr};
use stream_body::StreamBody;
use tokio::fs::File;

async fn handle(_: Request<Body>) -> Result<Response<StreamBody>, Infallible> {
    let f = File::open("large-file.pdf").await.unwrap();
    let file_size = f.metadata().await.unwrap().len();

    Ok(Response::builder()
        .header("Content-Type", "application/pdf")
        .header("Content-Length", file_size.to_string())
        .body(StreamBody::from_reader(f))
        .unwrap())
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
