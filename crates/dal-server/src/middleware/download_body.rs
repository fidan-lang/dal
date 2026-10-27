use axum::body::{Body, Bytes, HttpBody};
use http_body::{Frame, SizeHint};
use std::future::Future;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{sync::OwnedSemaphorePermit, time::Sleep};

/// Admission remains held until EOF, failure or client cancellation, not just headers.
pub struct DownloadBody {
    inner: Body,
    permit: Option<OwnedSemaphorePermit>,
    deadline: Pin<Box<Sleep>>,
    remaining: u64,
    done: bool,
}
impl DownloadBody {
    pub fn wrap(inner: Body, permit: OwnedSemaphorePermit, timeout: Duration, length: u64) -> Body {
        Body::new(Self {
            inner,
            permit: Some(permit),
            deadline: Box::pin(tokio::time::sleep(timeout)),
            remaining: length,
            done: false,
        })
    }
    fn finish(&mut self) {
        self.done = true;
        self.inner = Body::empty();
        self.permit.take();
    }
    fn failure(
        &mut self,
        message: &'static str,
    ) -> Poll<Option<Result<Frame<Bytes>, std::io::Error>>> {
        self.finish();
        Poll::Ready(Some(Err(std::io::Error::other(message))))
    }
}
impl HttpBody for DownloadBody {
    type Data = Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        if tokio::time::Instant::now() >= this.deadline.deadline()
            || this.deadline.as_mut().poll(cx).is_ready()
        {
            return this.failure("package download timed out");
        }
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    let Some(remaining) = this.remaining.checked_sub(data.len() as u64) else {
                        return this.failure("package download exceeded its declared size");
                    };
                    this.remaining = remaining;
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(_))) => this.failure("package download failed"),
            Poll::Ready(None) => {
                if this.remaining != 0 {
                    return this.failure("package download ended before its declared size");
                }
                this.finish();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }
    fn is_end_stream(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn body(
        value: &'static str,
        length: u64,
    ) -> (Body, std::sync::Arc<tokio::sync::Semaphore>) {
        let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let permit = slots.clone().try_acquire_owned().unwrap();
        (
            DownloadBody::wrap(Body::from(value), permit, Duration::from_secs(60), length),
            slots,
        )
    }
    #[tokio::test]
    async fn admission_is_held_through_body_and_released_on_completion_or_disconnect() {
        let (response, slots) = body("test", 4).await;
        assert!(slots.try_acquire().is_err());
        assert_eq!(
            axum::body::to_bytes(response, 16).await.unwrap().as_ref(),
            b"test"
        );
        assert_eq!(slots.available_permits(), 1);
        let (response, slots) = body("test", 4).await;
        drop(response);
        assert_eq!(slots.available_permits(), 1);
    }
    #[tokio::test]
    async fn truncation_overrun_and_timeout_fail_without_leaking_admission() {
        for length in [3, 5] {
            let (response, slots) = body("test", length).await;
            assert!(axum::body::to_bytes(response, 16).await.is_err());
            assert_eq!(slots.available_permits(), 1);
        }
        let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let response = DownloadBody::wrap(
            Body::from("test"),
            slots.clone().try_acquire_owned().unwrap(),
            Duration::ZERO,
            4,
        );
        tokio::task::yield_now().await;
        assert!(axum::body::to_bytes(response, 16).await.is_err());
        assert_eq!(slots.available_permits(), 1);
    }
}
