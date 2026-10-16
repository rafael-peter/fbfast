use super::*;
pub struct StreamWait {
    pub(super) shared: Option<Arc<Shared>>,
    pub(super) pid: u32,
    pub(super) action: StreamAction,
    pub(super) done: bool,
}

impl StreamWait {
    pub fn cancel(&self) {
        if self.pid == std::process::id() {
            self.shared.as_ref().expect("live stream").stop(true);
        }
    }
    pub fn budget(&self) -> Result<DeliveryBudget, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        self.shared
            .as_ref()
            .expect("live stream")
            .completion
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .delivery_budget()
    }
    pub fn poll(&mut self) -> Result<Option<StreamItem>, Error> {
        if self.pid != std::process::id() {
            return Err(fork_error());
        }
        let shared = self.shared.as_ref().expect("live stream");
        if self.done {
            return Ok(Some(StreamItem::End));
        }
        if self.action != StreamAction::Close {
            shared.check_terminal_error()?;
            if matches!(
                *shared.phase.lock().unwrap_or_else(|e| e.into_inner()),
                Phase::Ended | Phase::Closed
            ) {
                self.finish();
                return Ok(Some(StreamItem::End));
            }
            shared
                .completion
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending
                .check_waiting()?;
            self.budget()?.check_phase("stream_delivery")?;
        }
        loop {
            match shared.receiver.try_recv() {
                Ok(event) if self.action != StreamAction::Close => {
                    return self.accept(event).map(Some)
                }
                Ok(_) => continue, // close releases bounded prefetched batches
                Err(_) => break,
            }
        }
        if let Some(result) = shared.terminal() {
            if self.action == StreamAction::Close && result.is_ok() {
                shared
                    .completion
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .pending
                    .delivery_complete();
            }
            *shared.phase.lock().unwrap_or_else(|e| e.into_inner()) =
                if self.action == StreamAction::Close {
                    Phase::Closed
                } else {
                    Phase::Ended
                };
            self.finish();
            result.map(|()| Some(StreamItem::End))
        } else {
            Ok(None)
        }
    }
    pub async fn receive(&mut self) -> Result<Option<StreamItem>, Error> {
        if let Some(item) = self.poll()? {
            return Ok(Some(item));
        }
        let event = self
            .shared
            .as_ref()
            .expect("live stream")
            .receiver
            .recv_async()
            .await;
        match event {
            Ok(event) if self.action != StreamAction::Close => self.accept(event).map(Some),
            _ => self.poll(),
        }
    }
    fn accept(&mut self, event: Event) -> Result<StreamItem, Error> {
        self.shared
            .as_ref()
            .expect("live stream")
            .check_terminal_error()?;
        self.budget()?.check_phase("stream_delivery")?;
        let shared = self.shared.as_ref().expect("live stream");
        let item = match (self.action, event) {
            (StreamAction::Open, Event::Opened(schema)) => {
                *shared.schema.lock().unwrap_or_else(|e| e.into_inner()) = schema;
                *shared.phase.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Open;
                StreamItem::Opened
            }
            (StreamAction::Next, Event::Batch(rows)) => StreamItem::Batch(rows),
            (StreamAction::Next, Event::Arrow(batch)) => StreamItem::Arrow(batch),
            (StreamAction::Next, Event::Json(json)) => StreamItem::Json(json),
            _ => {
                return Err(Error::new(
                    ErrorKind::Query,
                    "stream",
                    "Unexpected stream protocol event",
                ))
            }
        };
        self.finish();
        Ok(item)
    }
    fn finish(&mut self) {
        self.done = true;
    }
}

impl Drop for StreamWait {
    fn drop(&mut self) {
        if self.pid != std::process::id() {
            if let Some(shared) = self.shared.take() {
                std::mem::forget(shared);
            }
        } else {
            let shared = self.shared.as_ref().expect("live stream");
            if !self.done {
                shared.stop(true);
            }
            shared.busy.store(false, Ordering::Release);
        }
    }
}
