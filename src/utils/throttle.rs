use std::sync::Arc;
use tokio::sync::Semaphore;

/// Simple throttling utility for managing concurrent operations
#[derive(Clone)]
pub struct Throttler {
    /// Semaphore for limiting concurrent operations
    semaphore: Arc<Semaphore>,
}

impl Throttler {
    /// Create a new throttler with specified limits
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Execute an operation with throttling
    pub async fn execute<F, Fut, T, E>(&self, operation: F) -> Result<T, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        // Acquire permit
        let _permit = self.semaphore.acquire().await;

        // Execute operation
        operation().await
    }

}

/// Network and Processing Manager for separating concerns
#[derive(Clone)]
pub struct NetworkProcessor {
    network_throttler: Option<Throttler>,
}

impl NetworkProcessor {
    /// Create a new network processor with custom limits
    pub fn new_with_limits(
        max_concurrent_network: usize,
        _max_concurrent_processing: usize,
    ) -> Self {
        let network_throttler = if max_concurrent_network > 0 {
            Some(Throttler::new(max_concurrent_network))
        } else {
            None
        };

        Self {
            network_throttler,
        }
    }

    /// Execute network operations with throttling
    pub async fn execute_network_operation<F, Fut, T, E>(&self, operation: F) -> Result<T, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        if let Some(ref throttler) = self.network_throttler {
            throttler.execute(operation).await
        } else {
            operation().await
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::Instant;

    #[tokio::test]
    async fn test_throttling_limits_concurrency() {
        let throttler = Throttler::new(2); // Max 2 concurrent
        let counter = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for _ in 0..5 {
            let throttler = throttler.clone();
            let counter = counter.clone();
            let max_concurrent = max_concurrent.clone();

            tasks.push(tokio::spawn(async move {
                throttler
                    .execute(|| async {
                        let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                        let _prev_max = max_concurrent.fetch_max(current, Ordering::SeqCst);

                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                        counter.fetch_sub(1, Ordering::SeqCst);
                        Ok::<(), String>(())
                    })
                    .await
            }));
        }

        for task in tasks {
            task.await.unwrap().unwrap();
        }

        // Should never exceed our concurrency limit of 2
        assert!(max_concurrent.load(Ordering::SeqCst) <= 2);
    }

}
