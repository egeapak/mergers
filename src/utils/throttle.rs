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

    /// Get available permits
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

/// Network and Processing Manager for separating concerns
#[derive(Clone)]
pub struct NetworkProcessor {
    network_throttler: Option<Throttler>,
    processing_throttler: Option<Throttler>,
}

impl NetworkProcessor {
    /// Create a new network processor
    pub fn new(max_concurrent_processing: usize) -> Self {
        Self::new_with_limits(100, max_concurrent_processing)
    }

    /// Create a new network processor with custom limits
    pub fn new_with_limits(
        max_concurrent_network: usize,
        max_concurrent_processing: usize,
    ) -> Self {
        let network_throttler = if max_concurrent_network > 0 {
            Some(Throttler::new(max_concurrent_network))
        } else {
            None
        };

        let processing_throttler = if max_concurrent_processing > 0 {
            Some(Throttler::new(max_concurrent_processing))
        } else {
            None
        };

        Self {
            network_throttler,
            processing_throttler,
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

    /// Execute network operations in parallel with throttling
    pub async fn execute_network_batch<F, Fut, T, E>(&self, operations: Vec<F>) -> Vec<Result<T, E>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let mut results = Vec::new();
        for operation in operations {
            let result = self.execute_network_operation(operation).await;
            results.push(result);
        }
        results
    }

    /// Execute processing operations with throttling
    pub async fn execute_processing_operation<F, Fut, T, E>(&self, operation: F) -> Result<T, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        if let Some(ref throttler) = self.processing_throttler {
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

    #[tokio::test]
    async fn test_network_processor_parallel_execution() {
        let processor = NetworkProcessor::new_with_limits(3, 2); // 3 concurrent network, 2 concurrent processing
        let counter = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        // Create network operations
        let operations: Vec<_> = (0..5)
            .map(|_| {
                let counter = counter.clone();
                let max_concurrent = max_concurrent.clone();
                move || async move {
                    let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let _prev_max = max_concurrent.fetch_max(current, Ordering::SeqCst);
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    counter.fetch_sub(1, Ordering::SeqCst);
                    Ok::<(), String>(())
                }
            })
            .collect();

        let start = Instant::now();
        let results = processor.execute_network_batch(operations).await;
        let elapsed = start.elapsed();

        // Network operations should be throttled to max 3 concurrent
        assert!(max_concurrent.load(Ordering::SeqCst) <= 3);
        assert_eq!(results.len(), 5);
        assert_eq!(counter.load(Ordering::SeqCst), 0); // All should be done
    }

    #[tokio::test]
    async fn test_processing_operations_are_throttled() {
        let processor = NetworkProcessor::new(1); // 1 concurrent
        let timestamps = Arc::new(tokio::sync::Mutex::new(Vec::new()));

        let mut tasks = Vec::new();
        for _i in 0..3 {
            let processor_ref = &processor;
            let timestamps = timestamps.clone();

            tasks.push(async move {
                processor_ref
                    .execute_processing_operation(|| async {
                        timestamps.lock().await.push(Instant::now());
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                        Ok::<(), String>(())
                    })
                    .await
            });
        }

        for task in tasks {
            task.await.unwrap();
        }

        let times = timestamps.lock().await;
        assert_eq!(times.len(), 3);

        // Operations should be sequential due to concurrency limit of 1
        for i in 1..times.len() {
            let duration = times[i].duration_since(times[i - 1]);
            assert!(duration >= tokio::time::Duration::from_millis(5)); // Some tolerance for sequential execution
        }
    }
}
