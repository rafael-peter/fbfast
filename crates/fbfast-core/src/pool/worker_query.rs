use super::{
    query::{Request, StreamFormat},
    state::{Inner, Job, Work},
    Output,
};
use crate::{
    error::{Error, ErrorKind},
    execution::{control::QueryOptions, ExecutionConnection},
};
use std::{sync::Weak, time::Instant};

pub(super) fn execute(
    inner: &Weak<Inner>,
    connection: &mut ExecutionConnection,
    job: &Job,
    end: Option<Instant>,
    cleaning: &dyn Fn() -> Result<bool, Error>,
) -> Result<Output, Error> {
    let options = |first, unique_names| QueryOptions {
        first,
        unique_names,
        prevalidated: true,
        deadline: end,
        cancellation: &job.progress.cancellation,
        cleaning,
    };
    match &job.work {
        Work::Query(query) => {
            let (sql, params) = (&query.sql, &query.params);
            match query.request {
                Request::Arrow(mode) => connection
                    .arrow_with(sql, params, mode, &options(false, true))
                    .map(Output::Arrow),
                Request::Json(json) => connection
                    .json_with(sql, params, json, &options(false, json.records))
                    .map(Output::Json),
                Request::Rows {
                    first,
                    unique_names,
                } => connection
                    .fetch_with(sql, params, &options(first, unique_names))
                    .map(Output::Rows),
                Request::Explain => connection
                    .explain_with(sql, params, &options(true, false))
                    .map(Output::Plan),
            }
        }
        Work::Stream {
            query,
            batch_rows,
            producer,
        } => {
            let mut consumer = super::stream_worker::Consumer {
                producer,
                inner,
                cancellation: &job.progress.cancellation,
                prefetch: connection.config.prefetch_batches,
                idle_timeout: connection.config.stream_idle_timeout,
                produced: 0,
                opened: false,
                deadline: end,
            };
            let (sql, params) = (&query.sql, &query.params);
            match query.format {
                StreamFormat::Arrow => connection
                    .stream_arrow_with(
                        sql,
                        params,
                        *batch_rows,
                        &options(false, true),
                        &mut consumer,
                    )
                    .map(|_| Output::Ready),
                StreamFormat::Json(json, format) => connection
                    .stream_json_with(
                        sql,
                        params,
                        *batch_rows,
                        (json, format),
                        &options(false, json.records),
                        &mut consumer,
                    )
                    .map(|_| Output::Ready),
                StreamFormat::Rows { unique_names } => connection
                    .stream_with(
                        sql,
                        params,
                        *batch_rows,
                        &options(false, unique_names),
                        &mut consumer,
                    )
                    .map(|_| Output::Ready),
            }
        }
        Work::Warmup | Work::KeepWarm => Ok(Output::Ready),
        Work::ClearCache { .. } | Work::Snapshot { .. } => Err(Error::new(
            ErrorKind::Query,
            "cache_clear",
            "Cache invalidation must use the coordinated path",
        )),
        #[cfg(test)]
        Work::Panic => panic!("intentional worker fault for lifecycle test"),
    }
}
