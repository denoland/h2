use ConnectionError;
use frame::{self, Frame};
use proto::{ReadySink, StreamMap, StreamTransporter, WindowSize};

use futures::*;

#[derive(Debug)]
pub struct FlowControl<T>  {
    inner: T,
}

impl<T, U> FlowControl<T>
    where T: Stream<Item = Frame, Error = ConnectionError>,
          T: Sink<SinkItem = Frame<U>, SinkError = ConnectionError>,
          T: StreamTransporter
{
    pub fn new(inner: T) -> FlowControl<T> {
        FlowControl { inner }
    }
}

impl<T: StreamTransporter> StreamTransporter for FlowControl<T> {
    fn streams(&self) -> &StreamMap {
        self.inner.streams()
    }

    fn streams_mut(&mut self) -> &mut StreamMap {
        self.inner.streams_mut()
    }
}

impl<T> Stream for FlowControl<T>
    where T: Stream<Item = Frame, Error = ConnectionError>,
          T: StreamTransporter,
 {
    type Item = T::Item;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<T::Item>, T::Error> {
        self.inner.poll()
    }
}


impl<T, U> Sink for FlowControl<T>
    where T: Sink<SinkItem = Frame<U>, SinkError = ConnectionError>,
          T: StreamTransporter,
 {
    type SinkItem = T::SinkItem;
    type SinkError = T::SinkError;

    fn start_send(&mut self, item: Frame<U>) -> StartSend<T::SinkItem, T::SinkError> {
        self.inner.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), T::SinkError> {
        self.inner.poll_complete()
    }
}

impl<T, U> ReadySink for FlowControl<T>
    where T: Stream<Item = Frame, Error = ConnectionError>,
          T: Sink<SinkItem = Frame<U>, SinkError = ConnectionError>,
          T: ReadySink,
          T: StreamTransporter,
{
    fn poll_ready(&mut self) -> Poll<(), ConnectionError> {
        self.inner.poll_ready()
    }
}
