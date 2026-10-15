use crate::{decode::DecodePlan, error::Error};
use fbfast_native::{Connection, Metadata, StatementId};
use std::collections::VecDeque;

pub(crate) struct Entry {
    pub sql: String,
    pub id: StatementId,
    pub input: Metadata,
    pub plan: DecodePlan,
}

impl Entry {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.sql.len()
            + self
                .input
                .columns
                .iter()
                .chain(&self.plan.schema.columns)
                .map(|c| std::mem::size_of_val(c) + c.name.len())
                .sum::<usize>()
    }
}

#[derive(Default)]
pub(crate) struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

impl Cache {
    pub fn take(&mut self, sql: &str) -> Option<Entry> {
        let entry = self
            .entries
            .iter()
            .position(|e| e.sql == sql)
            .and_then(|index| self.entries.remove(index));
        if let Some(entry) = &entry {
            self.bytes -= entry.bytes();
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        entry
    }

    pub fn put(
        &mut self,
        entry: Entry,
        native: &mut Connection,
        max_count: u32,
        max_bytes: u64,
    ) -> Result<(), Error> {
        let size = entry.bytes();
        if max_count == 0 || size as u64 > max_bytes {
            native.free_statement(entry.id)?;
            return Ok(());
        }
        while self.entries.len() >= max_count as usize || (self.bytes + size) as u64 > max_bytes {
            if let Some(evicted) = self.entries.pop_front() {
                self.bytes -= evicted.bytes();
                if let Err(error) = native.free_statement(evicted.id) {
                    // Native Connection still owns both handles if freeing fails.
                    let mut error = Error::from(error);
                    if let Err(cleanup) = native.free_statement(entry.id) {
                        error.cleanup.push(cleanup.into());
                    }
                    return Err(error);
                }
            } else {
                break;
            }
        }
        self.bytes += size;
        self.entries.push_back(entry);
        Ok(())
    }

    pub fn clear(&mut self, native: &mut Connection) -> Result<(), Error> {
        while let Some(entry) = self.entries.pop_front() {
            self.bytes -= entry.bytes();
            native.free_statement(entry.id)?;
        }
        Ok(())
    }
}
