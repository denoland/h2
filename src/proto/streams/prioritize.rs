use super::*;

#[derive(Debug)]
pub(super) struct Prioritize<B> {
    /// Streams that have pending frames
    pending_send: store::List<B>,

    /// Streams that are waiting for connection level flow control capacity
    pending_capacity: store::List<B>,

    /// Connection level flow control governing sent data
    flow_control: FlowControl,

    /// Total amount of buffered data in data frames
    buffered_data: usize,

    /// Holds frames that are waiting to be written to the socket
    buffer: Buffer<B>,

    /// Holds the connection task. This signals the connection that there is
    /// data to flush.
    conn_task: Option<task::Task>,
}

impl<B> Prioritize<B>
    where B: Buf,
{
    pub fn new(config: &Config) -> Prioritize<B> {
        Prioritize {
            pending_send: store::List::new(),
            pending_capacity: store::List::new(),
            flow_control: FlowControl::new(config.init_local_window_sz),
            buffered_data: 0,
            buffer: Buffer::new(),
            conn_task: None,
        }
    }

    pub fn available_window(&self) -> WindowSize {
        let win = self.flow_control.effective_window_size();

        if self.buffered_data >= win as usize {
            0
        } else {
            win - self.buffered_data as WindowSize
        }
    }

    pub fn recv_window_update(&mut self, frame: frame::WindowUpdate)
        -> Result<(), ConnectionError>
    {
        // Expand the window
        self.flow_control.expand_window(frame.size_increment())?;

        // Imediately apply the update
        self.flow_control.apply_window_update();

        Ok(())
    }

    pub fn queue_frame(&mut self,
                       frame: Frame<B>,
                       stream: &mut store::Ptr<B>)
    {
        self.buffered_data += frame.flow_len();

        // queue the frame in the buffer
        stream.pending_send.push_back(&mut self.buffer, frame);

        if stream.is_pending_send {
            debug_assert!(!self.pending_send.is_empty());

            // Already queued to have frame processed.
            return;
        }

        // Queue the stream
        push_sender(&mut self.pending_send, stream);

        if let Some(ref task) = self.conn_task {
            task.notify();
        }
    }

    pub fn poll_complete<T>(&mut self,
                            store: &mut Store<B>,
                            dst: &mut Codec<T, B>)
        -> Poll<(), ConnectionError>
        where T: AsyncWrite,
    {
        self.conn_task = Some(task::current());

        trace!("poll_complete");
        loop {
            // Ensure codec is ready
            try_ready!(dst.poll_ready());

            match self.pop_frame(store) {
                Some(frame) => {
                    trace!("writing frame={:?}", frame);
                    // Subtract the data size
                    self.buffered_data -= frame.flow_len();

                    let res = dst.start_send(frame)?;

                    // We already verified that `dst` is ready to accept the
                    // write
                    assert!(res.is_ready());
                }
                None => break,
            }
        }

        Ok(().into())
    }

    fn pop_frame(&mut self, store: &mut Store<B>) -> Option<Frame<B>> {
        loop {
            match self.pop_sender(store) {
                Some(mut stream) => {
                    let frame = match stream.pending_send.pop_front(&mut self.buffer).unwrap() {
                        Frame::Data(frame) => {
                            let len = frame.payload().remaining();

                            if len > self.flow_control.effective_window_size() as usize {
                                // TODO: This could be smarter...
                                stream.pending_send.push_front(&mut self.buffer, frame.into());

                                // Push the stream onto the list of streams
                                // waiting for connection capacity
                                push_sender(&mut self.pending_capacity, &mut stream);

                                // Try again w/ the next stream
                                continue;
                            }

                            frame.into()
                        }
                        frame => frame,
                    };

                    if !stream.pending_send.is_empty() {
                        push_sender(&mut self.pending_send, &mut stream);
                    }

                    return Some(frame);
                }
                None => return None,
            }
        }
    }

    fn pop_sender<'a>(&mut self, store: &'a mut Store<B>) -> Option<store::Ptr<'a, B>> {
        // If the connection level window has capacity, pop off of the pending
        // capacity list first.

        if self.flow_control.has_capacity() && !self.pending_capacity.is_empty() {
            let mut stream = self.pending_capacity
                .pop::<stream::Next>(store)
                .unwrap();

            stream.is_pending_send = false;
            Some(stream)
        } else {
            let stream = self.pending_send
                .pop::<stream::Next>(store);

            match stream {
                Some(mut stream) => {
                    stream.is_pending_send = false;
                    Some(stream)
                }
                None => None,
            }
        }
    }
}

fn push_sender<B>(list: &mut store::List<B>, stream: &mut store::Ptr<B>) {
    debug_assert!(!stream.is_pending_send);
    list.push::<stream::Next>(stream);
    stream.is_pending_send = true;
}
