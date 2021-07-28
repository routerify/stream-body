use crate::data::StreamData;
use crate::state::State;
use async_pipe::{self, PipeReader, PipeWriter};
use bytes::Bytes;
use http::{HeaderMap, HeaderValue};
use http_body::{Body, SizeHint};
use pin_project_lite::pin_project;
use std::borrow::Cow;
use std::marker::Unpin;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::{self, AsyncRead, ReadBuf};

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

/// An [HttpBody](https://docs.rs/hyper/0.14.11/hyper/body/trait.HttpBody.html) implementation which handles data streaming in an efficient way.
///
/// It is similar to [Body](https://docs.rs/hyper/0.14.11/hyper/body/struct.Body.html).
pub struct StreamBody {
    inner: Inner,
}

enum Inner {
    Once(OnceInner),
    Channel(ChannelInner),
}

struct OnceInner {
    data: Option<Bytes>,
    reached_eof: bool,
    state: Arc<Mutex<State>>,
}

pin_project! {
    struct ChannelInner {
        #[pin]
        reader: PipeReader,
        buf: Box<[u8]>,
        len: usize,
        reached_eof: bool,
        state: Arc<Mutex<State>>,
    }
}

impl StreamBody {
    /// Creates an empty body.
    pub fn empty() -> StreamBody {
        StreamBody {
            inner: Inner::Once(OnceInner {
                data: None,
                reached_eof: true,
                state: Arc::new(Mutex::new(State {
                    is_current_stream_data_consumed: true,
                    waker: None,
                })),
            }),
        }
    }

    /// Creates a body stream with an associated writer half.
    ///
    /// Useful when wanting to stream chunks from another thread.
    pub fn channel() -> (PipeWriter, StreamBody) {
        StreamBody::channel_with_capacity(DEFAULT_BUF_SIZE)
    }

    /// Creates a body stream with an associated writer half having a specific size of internal buffer.
    ///
    /// Useful when wanting to stream chunks from another thread.
    pub fn channel_with_capacity(capacity: usize) -> (PipeWriter, StreamBody) {
        let (w, r) = async_pipe::pipe();

        let mut buffer = Vec::with_capacity(capacity);
        unsafe {
            buffer.set_len(capacity);
        }

        let body = StreamBody {
            inner: Inner::Channel(ChannelInner {
                reader: r,
                buf: buffer.into_boxed_slice(),
                len: 0,
                reached_eof: false,
                state: Arc::new(Mutex::new(State {
                    is_current_stream_data_consumed: true,
                    waker: None,
                })),
            }),
        };

        (w, body)
    }

    /// A helper method to convert an [AsyncRead](https://docs.rs/tokio/0.2.16/tokio/io/trait.AsyncRead.html) to a `StreamBody`. If there is any error
    /// thrown during the reading/writing, it will be logged via [log::error!](https://docs.rs/log/0.4.10/log/macro.error.html).
    pub fn from_reader<R: AsyncRead + Unpin + Send + 'static>(mut r: R) -> StreamBody {
        let (mut w, body) = StreamBody::channel();

        tokio::spawn(async move {
            if let Err(err) = io::copy(&mut r, &mut w).await {
                log::error!(
                    "{}: StreamBody: Something went wrong while piping the provided reader to the body: {}",
                    env!("CARGO_PKG_NAME"),
                    err
                )
            }
        });

        body
    }
}

impl Body for StreamBody {
    type Data = StreamData;
    type Error = io::Error;

    fn poll_data(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.inner {
            Inner::Once(ref mut inner) => {
                let mut state;
                match inner.state.lock() {
                    Ok(s) => state = s,
                    Err(err) => {
                        return Poll::Ready(Some(Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!(
                                "{}: StreamBody [Once Data]: Failed to lock the stream state on poll data: {}",
                                env!("CARGO_PKG_NAME"),
                                err
                            ),
                        ))));
                    }
                }

                if !state.is_current_stream_data_consumed {
                    state.waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }

                if inner.reached_eof {
                    return Poll::Ready(None);
                }

                if let Some(ref bytes) = inner.data {
                    state.is_current_stream_data_consumed = false;
                    inner.reached_eof = true;

                    let data = StreamData::new(&bytes[..], Arc::clone(&inner.state));

                    return Poll::Ready(Some(Ok(data)));
                }

                return Poll::Ready(None);
            }
            Inner::Channel(ref mut inner) => {
                let mut inner_me = Pin::new(inner).project();

                let mut state;
                match inner_me.state.lock() {
                    Ok(s) => state = s,
                    Err(err) => {
                        return Poll::Ready(Some(Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!(
                                "{}: StreamBody [Channel Data]: Failed to lock the stream state on poll data: {}",
                                env!("CARGO_PKG_NAME"),
                                err
                            ),
                        ))));
                    }
                }

                if !state.is_current_stream_data_consumed {
                    state.waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }

                if *inner_me.reached_eof {
                    return Poll::Ready(None);
                }

                let mut buf = ReadBuf::new(&mut inner_me.buf);
                let poll_status = inner_me.reader.poll_read(cx, &mut buf);

                match poll_status {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(result) => match result {
                        Ok(_) => {
                            if (buf.capacity() - buf.remaining()) > 0 {
                                state.is_current_stream_data_consumed = false;

                                let data = StreamData::new(buf.filled(), Arc::clone(&inner_me.state));
                                Poll::Ready(Some(Ok(data)))
                            }else{
                                *inner_me.reached_eof = true;
                                Poll::Ready(None)
                            }
                        }
                        Err(err) => Poll::Ready(Some(Err(err))),
                    }
                }
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        match self.inner {
            Inner::Once(ref inner) => inner.reached_eof,
            Inner::Channel(ref inner) => inner.reached_eof,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self.inner {
            Inner::Once(ref inner) => match inner.data {
                Some(ref data) => SizeHint::with_exact(data.len() as u64),
                None => SizeHint::with_exact(0),
            },
            Inner::Channel(_) => SizeHint::default(),
        }
    }
}

impl From<Bytes> for StreamBody {
    #[inline]
    fn from(chunk: Bytes) -> StreamBody {
        if chunk.is_empty() {
            StreamBody::empty()
        } else {
            StreamBody {
                inner: Inner::Once(OnceInner {
                    data: Some(chunk),
                    reached_eof: false,
                    state: Arc::new(Mutex::new(State {
                        is_current_stream_data_consumed: true,
                        waker: None,
                    })),
                }),
            }
        }
    }
}

impl From<Vec<u8>> for StreamBody {
    #[inline]
    fn from(vec: Vec<u8>) -> StreamBody {
        StreamBody::from(Bytes::from(vec))
    }
}

impl From<&'static [u8]> for StreamBody {
    #[inline]
    fn from(slice: &'static [u8]) -> StreamBody {
        StreamBody::from(Bytes::from(slice))
    }
}

impl From<Cow<'static, [u8]>> for StreamBody {
    #[inline]
    fn from(cow: Cow<'static, [u8]>) -> StreamBody {
        match cow {
            Cow::Borrowed(b) => StreamBody::from(b),
            Cow::Owned(o) => StreamBody::from(o),
        }
    }
}

impl From<String> for StreamBody {
    #[inline]
    fn from(s: String) -> StreamBody {
        StreamBody::from(Bytes::from(s.into_bytes()))
    }
}

impl From<&'static str> for StreamBody {
    #[inline]
    fn from(slice: &'static str) -> StreamBody {
        StreamBody::from(Bytes::from(slice.as_bytes()))
    }
}

impl From<Cow<'static, str>> for StreamBody {
    #[inline]
    fn from(cow: Cow<'static, str>) -> StreamBody {
        match cow {
            Cow::Borrowed(b) => StreamBody::from(b),
            Cow::Owned(o) => StreamBody::from(o),
        }
    }
}
