use crate::state::State;
use hyper::body::Buf;
use std::sync::{Arc, Mutex};

pub struct StreamData {
    ptr: *const u8,
    len: usize,
    pos: usize,
    state: Arc<Mutex<State>>,
}

impl StreamData {
    pub(crate) fn new(s: &[u8], state: Arc<Mutex<State>>) -> StreamData {
        StreamData {
            ptr: s.as_ptr(),
            len: s.len(),
            pos: 0,
            state,
        }
    }
}

unsafe impl std::marker::Send for StreamData {}

impl Drop for StreamData {
    fn drop(&mut self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.is_current_stream_data_consumed = true;
            }
            Err(err) => log::error!(
                "{}: StreamData: Failed to update the drop state: {}",
                env!("CARGO_PKG_NAME"),
                err
            ),
        }
    }
}

impl Buf for StreamData {
    fn remaining(&self) -> usize {
        self.len - self.pos
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.add(self.pos), self.len - self.pos) }
    }

    fn advance(&mut self, cnt: usize) {
        self.pos += cnt;
    }
}
