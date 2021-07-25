# stream-body

[![crates.io](https://img.shields.io/crates/v/stream-body.svg)](https://crates.io/crates/stream-body)
[![Documentation](https://docs.rs/stream-body/badge.svg)](https://docs.rs/stream-body)
[![MIT](https://img.shields.io/crates/l/stream-body.svg)](./LICENSE)

An [HttpBody](https://docs.rs/hyper/0.14.11/hyper/body/trait.HttpBody.html) implementation with efficient streaming support for the Rust HTTP library [hyper](https://hyper.rs/).

[Docs](https://docs.rs/stream-body)

## Motivation

The existing [Body](https://docs.rs/hyper/0.14.11/hyper/body/struct.Body.html) type in [hyper](https://hyper.rs/) uses [Bytes](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html)
as streaming chunk. Hence, a lot of buffer allocation and de-allocation happen during the real-time large data streaming because of the [Bytes](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html) type.
Therefore, `StreamBody` comes to tackle this kind of situation. The `StreamBody` implements [HttpBody](https://docs.rs/hyper/0.14.11/hyper/body/trait.HttpBody.html) and uses `&[u8]`
slice as the streaming chunk, so it is possible to use the same buffer without allocating a new one; hence it overcomes any allocation/de-allocation overhead.

Also, the [channel()](https://docs.rs/hyper/0.14.11/hyper/body/struct.Body.html#method.channel) method in hyper [Body](https://docs.rs/hyper/0.14.11/hyper/body/struct.Body.html) returns
a pair of a [Sender](https://docs.rs/hyper/0.14.11/hyper/body/struct.Sender.html) and a [Body](https://docs.rs/hyper/0.14.11/hyper/body/struct.Body.html).
Here, the [Sender](https://docs.rs/hyper/0.14.11/hyper/body/struct.Sender.html) accepts [Bytes](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html) as a data chunk which again
creates allocation/de-allocation overhead.
To solve this, `StreamBody` has a method named `StreamBody::channel()` which returns a pair of an [AsyncWrite](https://docs.rs/tokio/0.2.16/tokio/io/trait.AsyncWrite.html) and the `StreamBody`
itself. As the [AsyncWrite](https://docs.rs/tokio/0.2.16/tokio/io/trait.AsyncWrite.html) accepts `&[u8]` instead of [Bytes](https://docs.rs/bytes/0.5.4/bytes/struct.Bytes.html), there will
be no allocation/de-allocation overhead.

## Usage

First add this to your Cargo.toml:

```toml
[dependencies]
stream-body = "0.1"
```

An example on handling a large file:

```rust
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::{convert::Infallible, net::SocketAddr};
use stream_body::StreamBody;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn handle(_: Request<Body>) -> Result<Response<StreamBody>, Infallible> {
    let (mut writer, body) = StreamBody::channel();

    tokio::spawn(async move {
        let mut f = File::open("large-file").await.unwrap();

        // Reuse this buffer
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
```

## Contributing

Your PRs and stars are always welcome.
