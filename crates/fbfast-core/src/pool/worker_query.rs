use super::{
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
        deadline: end,
        cancellation: &job.progress.cancellation,
        cleaning,
    };
    match &job.work {
        Work::Arrow {
            sql, params, mode, ..
        } => connection
            .arrow_with(sql, params, *mode, &options(false, true))
            .map(Output::Arrow),
        Work::Stream {
            sql,
            params,
            batch_rows,
            format,
            producer,
            ..
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
            if let super::stream::StreamFormat::Arrow = format {
                return connection
                    .stream_arrow_with(
                        sql,
                        params,
                        *batch_rows,
                        &options(false, true),
                        &mut consumer,
                    )
                    .map(|_| Output::Ready);
            }
            if let super::stream::StreamFormat::Json(json) = format {
                return connection
                    .stream_json_with(
                        sql,
                        params,
                        *batch_rows,
                        *json,
                        &options(false, json.0.records),
                        &mut consumer,
                    )
                    .map(|_| Output::Ready);
            }
            connection
                .stream_with(
                    sql,
                    params,
                    *batch_rows,
                    &options(
                        false,
                        matches!(format, super::stream::StreamFormat::Rows(true)),
                    ),
                    &mut consumer,
                )
                .map(|_| Output::Ready)
        }
        Work::Json {
            sql, params, json, ..
        } => connection
            .json_with(sql, params, *json, &options(false, json.records))
            .map(Output::Json),
        Work::Rows {
            sql,
            params,
            first,
            unique_names,
            ..
        } => connection
            .fetch_with(sql, params, &options(*first, *unique_names))
            .map(Output::Rows),
        Work::Explain { sql, params, .. } => connection
            .explain_with(sql, params, &options(true, false))
            .map(Output::Plan),
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
