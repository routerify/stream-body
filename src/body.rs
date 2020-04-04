use crate::data::StreamData;
use crate::state::State;
use hyper::body::Buf;
use hyper::{body::HttpBody, header::HeaderValue, HeaderMap};
use pin_project_lite::pin_project;
use std::io::BufReader;
use std::marker::PhantomData;
use std::marker::Unpin;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::{self, AsyncRead};

pub const DEFAULT_BUF_SIZE: usize = 8 * 1024;

pin_project! {
    pub struct StreamBody<R> {
        #[pin]
        pub(crate) reader: R,
        pub(crate) buf: Box<[u8]>,
        pub(crate) len: usize,
        pub(crate) reached_eof: bool,
        pub(crate) state: Arc<Mutex<State>>,
    }
}

impl<R> StreamBody<R>
where
    R: AsyncRead + Send + Sync,
{
    pub fn new(reader: R) -> StreamBody<R> {
        StreamBody::with_capacity(DEFAULT_BUF_SIZE, reader)
    }

    pub fn with_capacity(capacity: usize, reader: R) -> StreamBody<R> {
        unsafe {
            let mut buffer = Vec::with_capacity(capacity);
            buffer.set_len(capacity);

            {
                let b = &mut *(&mut buffer[..] as *mut [u8] as *mut [MaybeUninit<u8>]);
                reader.prepare_uninitialized_buffer(b);
            }

            StreamBody {
                reader,
                buf: buffer.into_boxed_slice(),
                len: 0,
                reached_eof: false,
                state: Arc::new(Mutex::new(State {
                    is_current_stream_data_consumed: true,
                })),
            }
        }
    }

    fn check_if_prev_data_consumed(&self) -> io::Result<()> {
        let mut state;
        match self.state.lock() {
            Ok(s) => state = s,
            Err(err) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "{}: StreamBody: Failed to lock the stream state: {}",
                        env!("CARGO_PKG_NAME"),
                        err
                    ),
                ))
            }
        }

        if !state.is_current_stream_data_consumed {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "The previous StreamData is not yet consumed",
            ));
        }

        Ok(())
    }
}

impl<R> HttpBody for StreamBody<R>
where
    R: AsyncRead + Send + Sync + Unpin,
{
    type Data = StreamData;
    type Error = io::Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        if let Err(err) = self.check_if_prev_data_consumed() {
            return Poll::Ready(Some(Err(err)));
        }

        let mut me = self.as_mut().project();
        let poll_status = me.reader.poll_read(cx, &mut me.buf[..]);

        match poll_status {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => match result {
                Ok(read_count) if read_count > 0 => {
                    let buf = &self.buf[..];
                    let data = StreamData::new(&self.buf[..], Arc::clone(&self.state));
                    Poll::Ready(Some(Ok(data)))
                }
                Ok(_) => {
                    self.reached_eof = true;
                    Poll::Ready(None)
                }
                Err(err) => Poll::Ready(Some(Err(err))),
            },
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        self.reached_eof
    }
}
