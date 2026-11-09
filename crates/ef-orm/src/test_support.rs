//! Shared test doubles: a driver that never touches a database.

use ef_orm_core::Value;
use ef_orm_driver::{Conn, Driver, DriverError, RowData, Tx};
use std::sync::Arc;

pub(crate) struct NoopConn;

impl Conn for NoopConn {
    type Tx<'a>
        = NoopTx
    where
        Self: 'a;

    async fn execute(&mut self, _: &str, _: &[Value]) -> Result<u64, DriverError> {
        Ok(0)
    }

    async fn query(&mut self, sql: &str, _: &[Value]) -> Result<Vec<RowData>, DriverError> {
        if sql.contains("health_check") {
            // Answer the pool's checkout probe.
            return Ok(vec![RowData::new(
                Arc::from(vec!["health_check".to_string()]),
                vec![Value::Int(1)],
            )]);
        }
        Ok(Vec::new())
    }

    async fn begin(&mut self) -> Result<NoopTx, DriverError> {
        Ok(NoopTx)
    }
}

pub(crate) struct NoopTx;

impl Tx for NoopTx {
    async fn execute(&mut self, _: &str, _: &[Value]) -> Result<u64, DriverError> {
        Ok(0)
    }

    async fn query(&mut self, _: &str, _: &[Value]) -> Result<Vec<RowData>, DriverError> {
        Ok(Vec::new())
    }

    async fn commit(self) -> Result<(), DriverError> {
        Ok(())
    }

    async fn rollback(self) -> Result<(), DriverError> {
        Ok(())
    }
}

pub(crate) struct NoopDriver;

impl Driver for NoopDriver {
    type Conn = NoopConn;

    async fn connect(&self) -> Result<NoopConn, DriverError> {
        Ok(NoopConn)
    }
}

/// A driver that answers every statement from canned data.
///
/// `rows` is echoed by `query` (identity `OUTPUT` reads), `affected` by
/// `execute` (0 exercises the optimistic-concurrency conflict path).
pub(crate) struct ScriptedDriver {
    pub rows: Vec<RowData>,
    pub affected: u64,
}

impl ScriptedDriver {
    pub fn new(rows: Vec<RowData>, affected: u64) -> Self {
        Self { rows, affected }
    }
}

impl Driver for ScriptedDriver {
    type Conn = ScriptedConn;

    async fn connect(&self) -> Result<ScriptedConn, DriverError> {
        Ok(ScriptedConn {
            rows: self.rows.clone(),
            affected: self.affected,
        })
    }
}

pub(crate) struct ScriptedConn {
    rows: Vec<RowData>,
    affected: u64,
}

impl Conn for ScriptedConn {
    type Tx<'a>
        = ScriptedTx
    where
        Self: 'a;

    async fn execute(&mut self, _: &str, _: &[Value]) -> Result<u64, DriverError> {
        Ok(self.affected)
    }

    async fn query(&mut self, sql: &str, _: &[Value]) -> Result<Vec<RowData>, DriverError> {
        Ok(if sql.contains("health_check") {
            vec![RowData::new(
                Arc::from(vec!["health_check".to_string()]),
                vec![Value::Int(1)],
            )]
        } else {
            self.rows.clone()
        })
    }

    async fn begin(&mut self) -> Result<ScriptedTx, DriverError> {
        Ok(ScriptedTx {
            rows: self.rows.clone(),
            affected: self.affected,
        })
    }

    async fn bulk_insert(
        &mut self,
        _table: &str,
        _columns: &[&str],
        rows: &[Vec<Value>],
    ) -> Result<u64, DriverError> {
        Ok(rows.len() as u64)
    }
}

pub(crate) struct ScriptedTx {
    rows: Vec<RowData>,
    affected: u64,
}

impl Tx for ScriptedTx {
    async fn execute(&mut self, _: &str, _: &[Value]) -> Result<u64, DriverError> {
        Ok(self.affected)
    }

    async fn query(&mut self, _: &str, _: &[Value]) -> Result<Vec<RowData>, DriverError> {
        Ok(self.rows.clone())
    }

    async fn commit(self) -> Result<(), DriverError> {
        Ok(())
    }

    async fn rollback(self) -> Result<(), DriverError> {
        Ok(())
    }
}
