use std::task::Waker;

pub(crate) struct State {
    pub(crate) is_current_stream_data_consumed: bool,
    pub(crate) waker: Option<Waker>,
}
