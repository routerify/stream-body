use hyper::service::{make_service_fn, service_fn};
use hyper::{server::Server, Body, Request, Response};
use std::{convert::Infallible, net::SocketAddr};
use stream_body::StreamBody;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn handle(_: Request<Body>) -> Result<Response<StreamBody>, Infallible> {
    let (mut writer, body) = StreamBody::channel();

    tokio::spawn(async move {
        let mut f = File::open("large-file").await.unwrap();

        let mut buf = [0_u8; 1024 * 16];
        loop {
            let read_count = f.read(&mut buf).await.unwrap();
            if read_count == 0 {
                break;
            }
            writer.write_all(&buf[..read_count]).await.unwrap();
        }
    });

    Ok(Response::builder().body(body).unwrap())
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
