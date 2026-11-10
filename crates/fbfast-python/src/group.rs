//! Several independent queries over one pool, at most `limit` at a time.
//!
//! Queries enter the pool queue in a window, so a large group never exceeds
//! the waiting capacity and other callers keep their place in the queue. The
//! consumer waits for whichever query finishes first with the GIL released;
//! each result is converted in the consumer's thread when it is taken, which
//! also bounds how many finished native results can wait for conversion.
use crate::{
    delivery::{finish, Form},
    errors::from_core,
    executor::Executor,
    values::Converter,
};
use fbfast_core::{
    decode::Value,
    error::{Error, ErrorKind},
    pool::{Pending, Pool, Query, Signal},
};
use pyo3::prelude::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Planned {
    index: usize,
    form: Form,
    sql: String,
    params: Vec<Value>,
    timeout: Option<f64>,
}

struct Running {
    index: usize,
    form: Form,
    pending: Pending,
}

struct State {
    queue: VecDeque<Planned>,
    running: Vec<Running>,
    /// Members whose submission failed, reported by the next `next` call.
    failed: VecDeque<Finished>,
    busy: bool,
    cancelled: bool,
}

#[pyclass(name = "_Group", module = "fbfast._native", frozen)]
pub struct PyGroup {
    pool: Pool,
    signal: Arc<Signal>,
    limit: usize,
    deadline: Option<Instant>,
    state: Mutex<State>,
}

/// One finished member: its position in the input, and either a value or an error.
type Finished = (usize, bool, Py<PyAny>);

/// (kind, sql, params, timeout, kind option) as sent by `fbfast.Query`.
pub type Spec<'py> = (
    String,
    String,
    Option<Bound<'py, PyAny>>,
    Option<f64>,
    Option<Bound<'py, PyAny>>,
);

enum Step {
    Done(Finished),
    Wait(u64),
    Empty,
}

impl PyGroup {
    pub fn new(
        py: Python<'_>,
        executor: &Executor,
        specs: Vec<Spec<'_>>,
        limit: usize,
        timeout: Option<f64>,
    ) -> PyResult<Self> {
        let Executor::Pool(pool) = executor else {
            return Err(from_core(Error::new(
                ErrorKind::UnsupportedFeature,
                "group",
                "Query groups run on a pool, not inside a snapshot",
            )));
        };
        let slots = pool.size().map_err(from_core)?.max(1);
        let deadline = match timeout {
            None => None,
            Some(seconds) => Some(
                Duration::try_from_secs_f64(seconds)
                    .ok()
                    .and_then(|duration| Instant::now().checked_add(duration))
                    .ok_or_else(|| {
                        from_core(Error::config(
                            "timeout",
                            "requires finite nonnegative seconds",
                        ))
                    })?,
            ),
        };
        let converter = Converter::new(py)?;
        let mut queue = VecDeque::with_capacity(specs.len());
        for (index, (kind, sql, params, query_timeout, option)) in specs.into_iter().enumerate() {
            queue.push_back(Planned {
                index,
                form: form(&kind, option.as_ref())?,
                sql,
                params: converter.params(params.as_ref())?,
                timeout: query_timeout,
            });
        }
        let group = Self {
            pool: pool.clone(),
            signal: Arc::new(Signal::default()),
            limit: limit.clamp(1, slots),
            deadline,
            state: Mutex::new(State {
                queue,
                running: Vec::new(),
                failed: VecDeque::new(),
                busy: false,
                cancelled: false,
            }),
        };
        // The first window starts now; the consumer may do other work meanwhile.
        group.fill(
            py,
            &mut group.state.lock().unwrap_or_else(|e| e.into_inner()),
        );
        Ok(group)
    }

    /// Submits queued members until `limit` are in flight.
    fn fill(&self, py: Python<'_>, state: &mut State) {
        while !state.cancelled && state.running.len() < self.limit {
            let Some(planned) = state.queue.pop_front() else {
                break;
            };
            let query = Query::new(
                planned.sql,
                planned.params,
                planned.timeout,
                planned.form.request(),
            );
            match self.pool.submit_grouped(query, &self.signal) {
                Ok(pending) => state.running.push(Running {
                    index: planned.index,
                    form: planned.form,
                    pending,
                }),
                Err(error) => {
                    let error = from_core(error).into_value(py).into_any();
                    state.failed.push_back((planned.index, false, error));
                }
            }
        }
    }

    fn step(&self, py: Python<'_>, state: &mut State) -> PyResult<Step> {
        if state.cancelled {
            return Ok(Step::Empty);
        }
        self.fill(py, state);
        if let Some(failed) = state.failed.pop_front() {
            return Ok(Step::Done(failed));
        }
        if state.running.is_empty() {
            return Ok(Step::Empty);
        }
        if self.deadline.is_some_and(|end| Instant::now() >= end) {
            cancel(state);
            return Err(from_core(Error::new(
                ErrorKind::QueryTimeout,
                "group",
                "Query group deadline expired; unfinished queries were cancelled",
            )));
        }
        let seen = self.signal.generation();
        for position in 0..state.running.len() {
            // Zero wait: also enforces each member's acquisition and query deadlines.
            match state.running[position].pending.wait_slice(Duration::ZERO) {
                Ok(None) => continue,
                Ok(Some(output)) => {
                    let member = state.running.swap_remove(position);
                    let mut converter = Converter::new(py)?;
                    let finished =
                        match finish(py, &mut converter, &member.pending, member.form, output) {
                            Ok(value) => (member.index, true, value),
                            Err(error) => (member.index, false, error.into_value(py).into_any()),
                        };
                    return Ok(Step::Done(finished));
                }
                Err(error) => {
                    let member = state.running.swap_remove(position);
                    let error = from_core(error).into_value(py).into_any();
                    return Ok(Step::Done((member.index, false, error)));
                }
            }
        }
        Ok(Step::Wait(seen))
    }
}

fn cancel(state: &mut State) {
    state.cancelled = true;
    state.queue.clear();
    state.failed.clear();
    for member in state.running.drain(..) {
        member.pending.cancel(false);
    }
}

#[pymethods]
impl PyGroup {
    /// The next finished member as (index, ok, value or exception); None when done.
    fn next(&self, py: Python<'_>) -> PyResult<Option<Finished>> {
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if std::mem::replace(&mut state.busy, true) {
                return Err(from_core(Error::new(
                    ErrorKind::ConcurrentUse,
                    "group",
                    "Query group is already being consumed by another caller",
                )));
            }
        }
        let result = (|| loop {
            let step = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                self.step(py, &mut state)?
            };
            match step {
                Step::Done(finished) => return Ok(Some(finished)),
                Step::Empty => return Ok(None),
                Step::Wait(seen) => {
                    let mut slice = Duration::from_millis(50);
                    if let Some(end) = self.deadline {
                        slice = slice.min(end.saturating_duration_since(Instant::now()));
                    }
                    let signal = self.signal.clone();
                    py.detach(|| signal.wait(seen, slice));
                    // KeyboardInterrupt leaves the group to be cancelled by its owner.
                    py.check_signals()?;
                }
            }
        })();
        self.state.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
        result
    }

    /// Cancels every unfinished member and drops the ones not yet submitted.
    fn cancel(&self) {
        cancel(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// Members not yet returned by `next` (queued, running or finished).
    fn remaining(&self) -> usize {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.cancelled {
            0
        } else {
            state.queue.len() + state.running.len() + state.failed.len()
        }
    }

    /// Effective concurrency after clamping to the pool size.
    #[getter]
    fn concurrency(&self) -> usize {
        self.limit
    }
}

fn form(kind: &str, option: Option<&Bound<'_, PyAny>>) -> PyResult<Form> {
    let required = || {
        option.ok_or_else(|| {
            from_core(Error::config(
                "group",
                "query option missing for this result kind",
            ))
        })
    };
    Ok(match kind {
        "rows" => {
            let (first, as_dict): (bool, bool) = required()?.extract()?;
            Form::Rows { first, as_dict }
        }
        "value" => Form::Value,
        "plan" => Form::Plan,
        "json" => Form::Json(crate::json::options(
            &required()?.extract::<Vec<String>>()?,
        )?),
        "arrow" => Form::Arrow(crate::arrow::mode(&required()?.extract::<String>()?)?),
        _ => {
            return Err(from_core(Error::config(
                "group",
                "unknown query result kind",
            )))
        }
    })
}
