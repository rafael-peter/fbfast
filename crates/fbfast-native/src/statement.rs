use crate::{ffi, Connection, NativeError};
use std::ffi::CString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatementId {
    pub(crate) owner: u64,
    pub(crate) id: u64,
}

impl Connection {
    pub(crate) fn validate_statement(&self, statement: StatementId) -> Result<u64, NativeError> {
        self.check()?;
        if statement.owner != self.owner || !self.statements.contains(&statement.id) {
            return Err(NativeError::InvalidInput(
                "Statement belongs to another attachment or has expired",
            ));
        }
        Ok(statement.id)
    }

    pub fn prepare(&mut self, sql: &str) -> Result<StatementId, NativeError> {
        self.check()?;
        let sql = CString::new(sql).map_err(|_| NativeError::InvalidInput("SQL contains NUL"))?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: C string survives call, handle exclusively owned, writable status.
        let id = unsafe { ffi::fbfast_prepare(self.handle.as_ptr(), sql.as_ptr(), &mut status) };
        status.check("prepare")?;
        if id == 0 {
            return Err(NativeError::InvalidInput(
                "Prepare returned an invalid statement",
            ));
        }
        self.statements.insert(id);
        Ok(StatementId {
            owner: self.owner,
            id,
        })
    }

    pub fn free_statement(&mut self, statement: StatementId) -> Result<(), NativeError> {
        let id = self.validate_statement(statement)?;
        let mut status = ffi::RawStatus::default();
        // SAFETY: statement is registered on this exclusively borrowed attachment.
        unsafe {
            ffi::fbfast_free_statement(self.handle.as_ptr(), id, &mut status);
        }
        status.check("free_statement")?;
        self.statements.remove(&id);
        Ok(())
    }

    pub fn plan(&mut self, statement: StatementId) -> Result<String, NativeError> {
        let id = self.validate_statement(statement)?;
        let mut size = 0;
        let mut status = ffi::RawStatus::default();
        // SAFETY: capacity zero requests length only; bridge does not dereference out.
        unsafe {
            ffi::fbfast_plan(
                self.handle.as_ptr(),
                id,
                std::ptr::null_mut(),
                0,
                &mut size,
                &mut status,
            );
        }
        status.check("plan")?;
        let mut bytes = vec![0u8; size as usize];
        // SAFETY: buffer has exactly the returned capacity; statement cannot mutate concurrently.
        unsafe {
            ffi::fbfast_plan(
                self.handle.as_ptr(),
                id,
                bytes.as_mut_ptr().cast(),
                size,
                &mut size,
                &mut status,
            );
        }
        status.check("plan")?;
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        String::from_utf8(bytes).map_err(|_| NativeError::InvalidInput("Plan text is not UTF8"))
    }
}
