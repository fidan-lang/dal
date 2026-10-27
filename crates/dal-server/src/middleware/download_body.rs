use axum::body::{Body, Bytes, HttpBody};
use http_body::{Frame, SizeHint};
use std::future::Future;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{sync::OwnedSemaphorePermit, time::Sleep};

/// Admission remains held until EOF, failure or client cancellation, not just headers.
pub struct DownloadBody {
    upstream: Arc<Mutex<Option<(Body, OwnedSemaphorePermit)>>>,
    cancellation: tokio::task::JoinHandle<()>,
    deadline: Pin<Box<Sleep>>,
    remaining: u64,
    done: bool,
}
impl DownloadBody {
    pub fn wrap(inner: Body, permit: OwnedSemaphorePermit, timeout: Duration, length: u64) -> Body {
        let deadline = tokio::time::Instant::now() + timeout;
        let upstream = Arc::new(Mutex::new(Some((inner, permit))));
        let weak = Arc::downgrade(&upstream);
        // Socket backpressure can stop Body polling entirely. Cancel the upstream
        // independently; otherwise a slow client can retain admission forever.
        let cancellation = tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(upstream) = weak.upgrade() {
                upstream.lock().expect("download state poisoned").take();
            }
        });
        Body::new(Self {
            upstream,
            cancellation,
            deadline: Box::pin(tokio::time::sleep_until(deadline)),
            remaining: length,
            done: false,
        })
    }
    fn finish(&mut self) {
        self.done = true;
        self.upstream
            .lock()
            .expect("download state poisoned")
            .take();
        self.cancellation.abort();
    }
    fn failure(
        &mut self,
        message: &'static str,
    ) -> Poll<Option<Result<Frame<Bytes>, std::io::Error>>> {
        self.finish();
        Poll::Ready(Some(Err(std::io::Error::other(message))))
    }
}
impl Drop for DownloadBody {
    fn drop(&mut self) {
        self.finish();
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
        let frame = this
            .upstream
            .lock()
            .expect("download state poisoned")
            .as_mut()
            .map(|(body, _)| Pin::new(body).poll_frame(cx));
        let Some(frame) = frame else {
            return this.failure("package download timed out");
        };
        match frame {
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
    async fn deadline_cancels_upstream_even_when_a_slow_client_stops_polling() {
        let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let response = DownloadBody::wrap(
            Body::from("held by backpressure"),
            slots.clone().try_acquire_owned().unwrap(),
            Duration::from_millis(10),
            20,
        );
        let permit = tokio::time::timeout(Duration::from_secs(1), slots.acquire())
            .await
            .expect("an unpolled response must not retain upstream admission forever")
            .unwrap();
        drop(permit);
        assert!(axum::body::to_bytes(response, 64).await.is_err());
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
