//! Minimal async pool: semaphore + `SELECT 1` health check + timeouts.
//!
//! No tokio type appears in the public API (roadmap risk "pool without tokio
//! in public types"): [`PooledConn`] hides the semaphore permit in a private
//! field and the whole surface is RPITIT-based.

use crate::{Conn, Driver, DriverError};
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::time::timeout;

/// Tuning knobs for [`Pool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolOptions {
    /// Maximum number of live connections (values below 1 clamp to 1).
    pub max_size: usize,
    /// Deadline for both acquiring a free slot and opening a new connection.
    pub connect_timeout: Duration,
}

impl Default for PoolOptions {
    fn default() -> Self {
        Self {
            max_size: 10,
            connect_timeout: Duration::from_secs(30),
        }
    }
}

/// A pool of reusable connections guarded by a semaphore.
///
/// Connections are validated with `SELECT 1 AS [health_check]` on every
/// checkout; stale ones are discarded and replaced. The connection is
/// returned to the idle list when [`PooledConn`] drops.
pub struct Pool<D: Driver> {
    driver: D,
    options: PoolOptions,
    permits: Semaphore,
    idle: Mutex<Vec<D::Conn>>,
}

impl<D: Driver> Pool<D> {
    /// Creates a pool capped at `options.max_size` live connections.
    pub fn new(driver: D, options: PoolOptions) -> Self {
        let max_size = options.max_size.max(1);
        Self {
            driver,
            options: PoolOptions {
                max_size,
                ..options
            },
            permits: Semaphore::new(max_size),
            idle: Mutex::new(Vec::new()),
        }
    }

    /// The effective options (after clamping `max_size`).
    pub fn options(&self) -> PoolOptions {
        self.options
    }

    /// Checks out a healthy connection, reusing an idle one when possible.
    ///
    /// Waits up to [`PoolOptions::connect_timeout`] for a free slot, then
    /// validates the candidate with a `SELECT 1` health check.
    pub async fn checkout(&self) -> Result<PooledConn<'_, D>, DriverError> {
        let permit = timeout(self.options.connect_timeout, self.permits.acquire())
            .await
            .map_err(|_| {
                DriverError::Timeout(format!(
                    "pool: no free connection after {:?}",
                    self.options.connect_timeout
                ))
            })?
            .map_err(|_| DriverError::Closed)?;

        let conn = self.healthy_connection().await?;
        Ok(PooledConn {
            pool: self,
            conn: Some(conn),
            _permit: permit,
        })
    }

    async fn healthy_connection(&self) -> Result<D::Conn, DriverError> {
        if let Some(mut reused) = self.take_idle() {
            if reused.ping().await.is_ok() {
                return Ok(reused);
            }
            // Stale: discarded (dropped here) and replaced below.
        }

        let mut fresh = timeout(self.options.connect_timeout, self.driver.connect())
            .await
            .map_err(|_| {
                DriverError::Timeout(format!(
                    "pool: connect timed out after {:?}",
                    self.options.connect_timeout
                ))
            })??;
        fresh.ping().await?;
        Ok(fresh)
    }

    fn take_idle(&self) -> Option<D::Conn> {
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
    }

    fn release(&self, conn: D::Conn) {
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(conn);
    }
}

/// A connection checked out from a [`Pool`]; returns to the pool on drop.
///
/// Derefs to the underlying connection, so `conn.query(...)` just works.
pub struct PooledConn<'a, D: Driver> {
    pool: &'a Pool<D>,
    conn: Option<D::Conn>,
    _permit: SemaphorePermit<'a>,
}

impl<D: Driver> Deref for PooledConn<'_, D> {
    type Target = D::Conn;

    fn deref(&self) -> &Self::Target {
        // Invariant: `conn` is only taken in `Drop`, which runs after the
        // last use of this guard, so it is always present here.
        self.conn
            .as_ref()
            .expect("PooledConn accessed after being dropped")
    }
}

impl<D: Driver> DerefMut for PooledConn<'_, D> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("PooledConn accessed after being dropped")
    }
}

impl<D: Driver> Drop for PooledConn<'_, D> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.release(conn);
        }
        // `_permit` drops right after, freeing the semaphore slot.
    }
}

#[cfg(test)]
mod tests {
    use super::{Pool, PoolOptions};
    use crate::{Conn, Driver, DriverError, RowData, Tx};
    use ef_orm_core::Value;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::time::timeout;

    #[derive(Clone)]
    struct MockDriver {
        connects: Arc<AtomicUsize>,
    }

    impl MockDriver {
        fn new() -> Self {
            Self {
                connects: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Driver for MockDriver {
        type Conn = MockConn;

        async fn connect(&self) -> Result<MockConn, DriverError> {
            self.connects.fetch_add(1, Ordering::SeqCst);
            Ok(MockConn { stale: false })
        }
    }

    struct MockConn {
        stale: bool,
    }

    impl Conn for MockConn {
        type Tx<'a>
            = MockTx<'a>
        where
            Self: 'a;

        async fn execute(&mut self, _sql: &str, _params: &[Value]) -> Result<u64, DriverError> {
            Ok(0)
        }

        async fn query(
            &mut self,
            sql: &str,
            _params: &[Value],
        ) -> Result<Vec<RowData>, DriverError> {
            if sql.contains("health_check") {
                if self.stale {
                    return Err(DriverError::Server {
                        code: None,
                        message: "stale connection".into(),
                    });
                }
                // Echo what the real health check expects back: SELECT 1.
                return Ok(vec![RowData::new(
                    Arc::from(vec!["health_check".to_string()]),
                    vec![Value::Int(1)],
                )]);
            }
            Ok(Vec::new())
        }

        async fn begin(&mut self) -> Result<MockTx<'_>, DriverError> {
            Ok(MockTx { conn: self })
        }
    }

    struct MockTx<'a> {
        conn: &'a mut MockConn,
    }

    impl Tx for MockTx<'_> {
        async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<u64, DriverError> {
            self.conn.execute(sql, params).await
        }

        async fn query(
            &mut self,
            sql: &str,
            params: &[Value],
        ) -> Result<Vec<RowData>, DriverError> {
            self.conn.query(sql, params).await
        }

        async fn commit(self) -> Result<(), DriverError> {
            self.conn.stale = false;
            Ok(())
        }

        async fn rollback(self) -> Result<(), DriverError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn checkout_reuses_the_idle_connection() {
        let driver = MockDriver::new();
        let connects = Arc::clone(&driver.connects);
        let pool = Pool::new(driver, PoolOptions::default());

        drop(pool.checkout().await.unwrap());
        drop(pool.checkout().await.unwrap());

        assert_eq!(connects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn checkout_times_out_when_every_slot_is_busy() {
        let driver = MockDriver::new();
        let pool = Pool::new(
            driver,
            PoolOptions {
                max_size: 1,
                connect_timeout: Duration::from_secs(5),
            },
        );

        let held = pool.checkout().await.unwrap();
        assert!(
            timeout(Duration::from_millis(50), pool.checkout())
                .await
                .is_err(),
            "second checkout must not get a slot while the first is held"
        );

        drop(held);
        assert!(pool.checkout().await.is_ok());
    }

    #[tokio::test]
    async fn stale_connections_are_replaced_on_checkout() {
        let driver = MockDriver::new();
        let connects = Arc::clone(&driver.connects);
        let pool = Pool::new(driver, PoolOptions::default());

        let mut conn = pool.checkout().await.unwrap();
        conn.stale = true;
        drop(conn);

        assert!(pool.checkout().await.is_ok());
        assert_eq!(
            connects.load(Ordering::SeqCst),
            2,
            "failed health check must force a fresh connection"
        );
    }
}
